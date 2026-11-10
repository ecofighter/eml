// QtのbackendのC++の側 (docs/implementation/architecture.mdの「`eml_gui`の内部」)。Qtのcallbackは`Events`に積むか、
// ループの印を立てるだけで、Rustの`step`を呼ばない。
#include "eml_gui/src/qt/shim.hpp"
#include "eml_gui/src/qt/mod.rs.h"

#include <QtCore/QAbstractEventDispatcher>
#include <QtCore/QSignalBlocker>
#include <QtGui/QCloseEvent>
#include <QtGui/QResizeEvent>
#include <QtWidgets/QCheckBox>
#include <QtWidgets/QComboBox>
#include <QtWidgets/QLineEdit>
#include <QtWidgets/QProgressBar>
#include <QtWidgets/QPushButton>

#include <algorithm>
#include <climits>
#include <clocale>
#include <cmath>
#include <string>
#include <utility>
#include <vector>

#include <sys/syscall.h>
#include <unistd.h>

namespace eml_gui {
namespace {

QString qstring(rust::Str text) {
  return QString::fromUtf8(text.data(), static_cast<qsizetype>(text.size()));
}

// ボタンの文字の`&`はニーモニックの印なので、そのまま見せるために重ねる
QString button_text(rust::Str text) {
  return qstring(text).replace(QLatin1Char('&'), QLatin1String("&&"));
}

bool is_window(std::uint64_t id) { return (id & 0xffffffffu) == 0; }

int round_up(double v) { return static_cast<int>(std::ceil(v)); }

// 閉じるボタンは閉じる要求を積むだけで、ウィンドウを閉じるのはプログラムである。大きさの変化と、字体やスタイルの
// 変化も積む
class Window : public QWidget {
public:
  Window(const Events &events, std::uint64_t id) : events(events), id(id) {}

protected:
  void closeEvent(QCloseEvent *event) override {
    event->ignore();
    events.close_request(id);
  }

  void resizeEvent(QResizeEvent *event) override {
    QWidget::resizeEvent(event);
    events.resized(id, event->size().width(), event->size().height());
  }

  void changeEvent(QEvent *event) override {
    QWidget::changeEvent(event);
    if (event->type() == QEvent::FontChange ||
        event->type() == QEvent::StyleChange) {
      events.invalidate(id);
    }
  }

private:
  const Events &events;
  std::uint64_t id;
};

// QApplicationの構築は`setlocale(LC_ALL, "")`でプロセスのロケールをシステムのものに替える。Cのコードのロケールが
// GUIを開いたことで変わらないように、構築の前の値に戻す。ただしLC_CTYPEはQtが選んだもの (UTF-8) を残す。QtはUTF-8の
// ロケールを前提とし、UTF-8でなければ自分でC.UTF-8に切り替えるからである
std::unique_ptr<QApplication> new_app(int &argc, char **argv) {
  std::string before = std::setlocale(LC_ALL, nullptr);
  auto app = std::make_unique<QApplication>(argc, argv);
  std::string ctype = std::setlocale(LC_CTYPE, nullptr);
  std::setlocale(LC_ALL, before.c_str());
  std::setlocale(LC_CTYPE, ctype.c_str());
  return app;
}

Hints hints_of(QSize min, QSize size) {
  return Hints{static_cast<double>(std::max(min.width(), 0)),
               static_cast<double>(std::max(min.height(), 0)),
               static_cast<double>(std::max(size.width(), 0)),
               static_cast<double>(std::max(size.height(), 0))};
}

} // namespace

Shim::Shim(rust::Box<Events> events)
    : argc(1), arg0{'e', 'm', 'l', '\0'}, argv{arg0, nullptr},
      app(new_app(argc, argv)),
      events(std::move(events)), measurer(std::make_unique<QLabel>()),
      timer(std::make_unique<QTimer>()) {
  // ウィンドウをすべて閉じてもループは終わらない。終えるのはrunだけである (docs/spec/vm.mdの「GUIのPark」)
  QApplication::setQuitOnLastWindowClosed(false);
  measurer->setTextFormat(Qt::PlainText);
  timer->setSingleShot(true);
  timer->setTimerType(Qt::PreciseTimer);
  QObject::connect(timer.get(), &QTimer::timeout, timer.get(),
                   [this] { timed_out = true; });
}

Shim::~Shim() {
  notifier.reset();
  std::vector<QWidget *> windows;
  for (const auto &[id, widget] : widgets) {
    if (is_window(id) && widget) {
      windows.push_back(widget.data());
    }
  }
  for (QWidget *window : windows) {
    delete window;
  }
}

QWidget *Shim::find(std::uint64_t id) const {
  auto it = widgets.find(id);
  return it == widgets.end() ? nullptr : it->second.data();
}

void Shim::create_window(std::uint64_t id, rust::Str title) const {
  auto *window = new Window(*events, id);
  window->setWindowTitle(qstring(title));
  widgets[id] = window;
}

void Shim::create(std::uint64_t id, WidgetKind kind, std::uint64_t parent,
                  rust::Str text, std::int64_t value) const {
  QWidget *into = find(parent);
  if (into == nullptr) {
    return;
  }
  const Events *sink = &*events;
  QWidget *widget = nullptr;
  switch (kind) {
  case WidgetKind::Grid:
    widget = new QWidget(into);
    break;
  case WidgetKind::Label: {
    auto *label = new QLabel(into);
    label->setTextFormat(Qt::PlainText);
    label->setWordWrap(true);
    label->setText(qstring(text));
    widget = label;
    break;
  }
  case WidgetKind::Button: {
    auto *button = new QPushButton(button_text(text), into);
    QObject::connect(button, &QPushButton::clicked, button,
                     [sink, id] { sink->click(id); });
    widget = button;
    break;
  }
  case WidgetKind::TextField: {
    auto *field = new QLineEdit(qstring(text), into);
    // `textEdited`は打ったときだけ出て、`setText`では出ない
    QObject::connect(field, &QLineEdit::textEdited, field,
                     [sink, id](const QString &now) {
                       QByteArray utf8 = now.toUtf8();
                       sink->text(id, rust::Str(utf8.constData(),
                                                static_cast<std::size_t>(utf8.size())));
                     });
    widget = field;
    break;
  }
  case WidgetKind::CheckBox: {
    auto *check = new QCheckBox(button_text(text), into);
    check->setChecked(value != 0);
    // `clicked`は操作したときだけ出て、`setChecked`では出ない
    QObject::connect(check, &QCheckBox::clicked, check,
                     [sink, id](bool on) { sink->toggle(id, on); });
    widget = check;
    break;
  }
  case WidgetKind::Choice: {
    auto *choice = new QComboBox(into);
    // `activated`は選んだときだけ出て、項目を足すことや`setCurrentIndex`では出ない
    QObject::connect(choice, &QComboBox::activated, choice,
                     [sink, id](int index) { sink->select(id, index); });
    widget = choice;
    break;
  }
  case WidgetKind::ProgressBar: {
    auto *bar = new QProgressBar(into);
    bar->setRange(0, 100);
    bar->setValue(static_cast<int>(value));
    widget = bar;
    break;
  }
  }
  if (widget == nullptr) {
    return;
  }
  // 表示した後の親に足した子は、自分では表示されない
  widget->show();
  widgets[id] = widget;
}

void Shim::add_item(std::uint64_t id, rust::Str item) const {
  if (auto *choice = qobject_cast<QComboBox *>(find(id))) {
    QSignalBlocker blocker(choice);
    choice->addItem(qstring(item));
  }
}

void Shim::set_text(std::uint64_t id, rust::Str text) const {
  QWidget *widget = find(id);
  if (widget == nullptr) {
    return;
  }
  QSignalBlocker blocker(widget);
  if (is_window(id)) {
    widget->setWindowTitle(qstring(text));
  } else if (auto *label = qobject_cast<QLabel *>(widget)) {
    label->setText(qstring(text));
  } else if (auto *field = qobject_cast<QLineEdit *>(widget)) {
    field->setText(qstring(text));
  } else if (auto *button = qobject_cast<QAbstractButton *>(widget)) {
    button->setText(button_text(text));
  }
}

void Shim::set_value(std::uint64_t id, std::int64_t value) const {
  QWidget *widget = find(id);
  if (widget == nullptr) {
    return;
  }
  QSignalBlocker blocker(widget);
  if (auto *bar = qobject_cast<QProgressBar *>(widget)) {
    bar->setValue(static_cast<int>(value));
  } else if (auto *choice = qobject_cast<QComboBox *>(widget)) {
    choice->setCurrentIndex(static_cast<int>(value));
  }
}

void Shim::set_checked(std::uint64_t id, bool on) const {
  if (auto *check = qobject_cast<QCheckBox *>(find(id))) {
    QSignalBlocker blocker(check);
    check->setChecked(on);
  }
}

void Shim::set_enabled(std::uint64_t id, bool on) const {
  if (QWidget *widget = find(id)) {
    widget->setEnabled(on);
  }
}

Hints Shim::hints(std::uint64_t id) const {
  QWidget *widget = find(id);
  if (widget == nullptr) {
    return Hints{0, 0, 0, 0};
  }
  return hints_of(widget->minimumSizeHint(), widget->sizeHint());
}

// 折り返すラベルの`sizeHint`は縦横の比で決まる幅なので、折り返さないラベルで1行の大きさを測る
Hints Shim::one_line(std::uint64_t id) const {
  auto *label = qobject_cast<QLabel *>(find(id));
  if (label == nullptr) {
    return Hints{0, 0, 0, 0};
  }
  measurer->setFont(label->font());
  measurer->setText(label->text());
  return hints_of(measurer->minimumSizeHint(), measurer->sizeHint());
}

double Shim::height_for_width(std::uint64_t id, double width) const {
  QWidget *widget = find(id);
  if (widget == nullptr) {
    return 0;
  }
  if (!widget->hasHeightForWidth()) {
    return std::max(widget->sizeHint().height(), 0);
  }
  return std::max(widget->heightForWidth(round_up(width)), 0);
}

// 端を丸めるので、隣り合う升目の間に隙間も重なりもできない
void Shim::place(std::uint64_t id, double x, double y, double w,
                 double h) const {
  if (QWidget *widget = find(id)) {
    int left = static_cast<int>(std::lround(x));
    int top = static_cast<int>(std::lround(y));
    int right = static_cast<int>(std::lround(x + w));
    int bottom = static_cast<int>(std::lround(y + h));
    widget->setGeometry(left, top, right - left, bottom - top);
  }
}

void Shim::show_window(std::uint64_t id, double w, double h, double min_w,
                       double min_h) const {
  if (QWidget *window = find(id)) {
    window->setMinimumSize(round_up(min_w), round_up(min_h));
    window->resize(round_up(w), round_up(h));
    // 前に出すのはウィンドウマネージャーに任せる。Waylandでは`activateWindow`が効かず、警告を書く
    window->show();
  }
}

void Shim::destroy(std::uint64_t id) const {
  QWidget *widget = find(id);
  if (is_window(id)) {
    std::uint64_t window_no = id >> 32;
    for (auto it = widgets.begin(); it != widgets.end();) {
      it = (it->first >> 32) == window_no ? widgets.erase(it) : std::next(it);
    }
  } else {
    widgets.erase(id);
  }
  if (widget != nullptr) {
    widget->hide();
    delete widget;
  }
}

std::size_t Shim::dispatcher() const {
  return reinterpret_cast<std::size_t>(QAbstractEventDispatcher::instance());
}

void Shim::watch(int fd) const {
  notifier = std::make_unique<QSocketNotifier>(fd, QSocketNotifier::Read);
  QObject::connect(notifier.get(), &QSocketNotifier::activated, notifier.get(),
                   [this] { io_ready = true; });
}

void Shim::unwatch() const {
  notifier.reset();
  timer->stop();
  io_ready = false;
  timed_out = false;
}

void Shim::arm(std::int64_t ms) const {
  if (ms < 0) {
    timer->stop();
  } else {
    timer->start(static_cast<int>(std::min<std::int64_t>(ms, INT_MAX)));
  }
}

void Shim::process(bool wait) const {
  QCoreApplication::processEvents(wait ? QEventLoop::WaitForMoreEvents
                                       : QEventLoop::AllEvents);
  // `exec`のイベントループを回さないので、Qtの部品が`deleteLater`で頼んだ後始末はここで流す
  QCoreApplication::sendPostedEvents(nullptr, QEvent::DeferredDelete);
}

bool Shim::take_io() const { return std::exchange(io_ready, false); }

bool Shim::take_timeout() const { return std::exchange(timed_out, false); }

bool on_main_thread() { return ::syscall(SYS_gettid) == ::getpid(); }

bool app_exists() { return QCoreApplication::instance() != nullptr; }

std::unique_ptr<Shim> new_shim(rust::Box<Events> events) {
  return std::make_unique<Shim>(std::move(events));
}

void wake(std::size_t dispatcher) {
  reinterpret_cast<QAbstractEventDispatcher *>(dispatcher)->wakeUp();
}

} // namespace eml_gui
