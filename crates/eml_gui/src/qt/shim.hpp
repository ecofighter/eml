// QtのbackendのC++の側。Rustからはcxxの橋 (`mod.rs`) を通して呼ぶ (docs/implementation/architecture.mdの「`eml_gui`の内部」)。
#pragma once

#include <QtCore/QPointer>
#include <QtCore/QSocketNotifier>
#include <QtCore/QTimer>
#include <QtWidgets/QApplication>
#include <QtWidgets/QLabel>
#include <QtWidgets/QWidget>

#include <cstddef>
#include <cstdint>
#include <memory>
#include <unordered_map>

#include "rust/cxx.h"

namespace eml_gui {

// cxxが生成するヘッダーで定める型。このヘッダーはその前に読まれる
struct Events;
struct Hints;
enum class WidgetKind : std::uint8_t;

// QApplicationと、`Id`で引くウィジェットの表を持つ。メインのスレッドだけで使う。
class Shim {
public:
  explicit Shim(rust::Box<Events> events);
  ~Shim();
  Shim(const Shim &) = delete;
  Shim &operator=(const Shim &) = delete;

  void create_window(std::uint64_t id, rust::Str title) const;
  void create(std::uint64_t id, WidgetKind kind, std::uint64_t parent,
              rust::Str text, std::int64_t value) const;
  void add_item(std::uint64_t id, rust::Str item) const;
  void set_text(std::uint64_t id, rust::Str text) const;
  void set_value(std::uint64_t id, std::int64_t value) const;
  void set_checked(std::uint64_t id, bool on) const;
  void set_enabled(std::uint64_t id, bool on) const;
  Hints hints(std::uint64_t id) const;
  Hints one_line(std::uint64_t id) const;
  double height_for_width(std::uint64_t id, double width) const;
  void place(std::uint64_t id, double x, double y, double w, double h) const;
  void show_window(std::uint64_t id, double w, double h, double min_w,
                   double min_h) const;
  void destroy(std::uint64_t id) const;

  std::size_t dispatcher() const;
  void watch(int fd) const;
  void unwatch() const;
  void arm(std::int64_t ms) const;
  void process(bool wait) const;
  bool take_io() const;
  bool take_timeout() const;

private:
  QWidget *find(std::uint64_t id) const;

  // QApplicationはargcとargvを参照で持ち続けるので、先に宣言してQApplicationより長く生かす
  int argc;
  char arg0[4];
  char *argv[2];
  std::unique_ptr<QApplication> app;
  rust::Box<Events> events;
  // 1行のラベルの大きさを測るための、画面に出さないラベル
  std::unique_ptr<QLabel> measurer;
  std::unique_ptr<QTimer> timer;
  mutable std::unique_ptr<QSocketNotifier> notifier;
  mutable bool io_ready = false;
  mutable bool timed_out = false;
  // ウィンドウを壊すと子も壊れるので、残った子の項目はQPointerで空になる
  mutable std::unordered_map<std::uint64_t, QPointer<QWidget>> widgets;
};

bool on_main_thread();
bool app_exists();
std::unique_ptr<Shim> new_shim(rust::Box<Events> events);
void wake(std::size_t dispatcher);

} // namespace eml_gui
