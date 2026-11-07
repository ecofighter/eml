# 配布とGUIの前提

位置づけ: 手引き。

ネイティブのGUIでemlを動かす利用者が何を入れておくか、emlを組む人が何を用意するか、emlやそれから作ったものを配る人がGUIのライブラリのライセンスの下で何を負うかを定める。GUIのbackendの中身は [アーキテクチャ](architecture.md) の「`eml_gui`の内部」、`Std.Gui`の規則は [標準ライブラリ](../spec/stdlib.md) の「`Std.Gui`」にある。

## 動かすのに要るもの

| OS | 利用者が用意するもの |
|---|---|
| macOS | なし。AppKitとFoundationはOSの一部である |
| Linux | `nix build`で組んだ`eml`はNixのclosureにQt 6を持ち、`wrapQtAppsHook`が作るラッパーがQtのプラグインの場所を置くので、ほかに要らない。Nixを使わないときは、`libQt6Core.so.6`、`libQt6Gui.so.6`、`libQt6Widgets.so.6`、ディストリビューションのQtのプラットフォームプラグイン (`qt6-qpa-plugins`など)、X11かWaylandの画面 (`DISPLAY`か`WAYLAND_DISPLAY`) |
| Windows | Windows 11 (build 22000以降)、利用者がMicrosoftから入れたWindows App SDK 2のruntime、Visual C++の再頒布可能パッケージ |

- `gui-native`を足して組んだ`eml`のLinux版は、Qtを起動のときに読み込むので、GUIを使わないプログラムを動かすときもQtのライブラリが要る。Windowsは、プログラムが初めてGUIを求めたときに、Windows App SDKのruntimeをその場で探す。macOSはAppKitを起動のときにつなぐ
- `gui-native`なしで組んだ`eml`は、上の表のGUIのtoolkitの項目を要らない。nativeのbackendを持たず、`--gui stub`と同じく画面を開かないからである ([標準ライブラリ](../spec/stdlib.md) の「`Std.Gui`」)
- 足りないものがあると、`Gui.run`が`NotAvailable`か`Other`で失敗する。Windows 11より古いときと、Windows App SDKのruntimeがないときの文言は、Microsoftの配布のページ (https://learn.microsoft.com/windows/apps/windows-app-sdk/downloads) を示す。Linuxの画面がないときの文言は`Other`で、QApplicationを作る前に出る。文言は [標準ライブラリ](../spec/stdlib.md) の「`Std.Gui`」の誤りの表にある
- Qtのプラットフォームプラグインの探し方は、Nixのラッパーが置く場所に頼る。Nixの外では、ディストリビューションが入れるプラグインに頼る

## 組むのに要るもの

nativeのbackendはfeature `gui-native`が足し、既定のビルドはGUIのtoolkitのファイルを要らない ([アーキテクチャ](architecture.md) の「外部 crate」)。

- macOS: AppKitとFoundationをつなぐクレートはRustだけで、C++のコンパイラもtoolkitのファイルも要らない
- Linux: Qt 6の開発用のファイル (ヘッダと`.prl`) と、C++17のコンパイラが要る。Qtは`QMAKE`のqmake、なければ`PATH`の`qmake6`か`qmake`で探し、Qt 5が見つかると`build.rs`がpanicする。Qtのライブラリは、`libQt6Core.so.6`のように版を持つsonameで動的につなぐ。Nixでは`flake.nix`のdevShellとパッケージがqtbase、pkg-config、`wrapQtAppsHook`を足し、x86_64 Linuxではnixpkgsのlinkerのラッパーを通してつなぐので、Qtとlibstdc++へのRUNPATHがバイナリに入る ([テスト戦略](testing.md) の「CI」)
- Windows: Windows App SDKのファイル (import library、`.winmd`、bootstrapのDLL、インストーラ) は、組むときも要らず、配るものにも入らない。WinRTとuser32とntdllの関数は`raw-dylib`でつなぐ。Rustの通常のMSVCのtoolchainがあればよい

## Qtを配る規則

Qtは、`eml`でも、これから作るネイティブの出力でも、LGPL-3.0に従って動的にだけつなぐ。静的にリンクすると、利用者がQtを差し替えて結び直せるようにする義務が配る側に生じ、守るのも確かめるのも重くなるため採らない。

emlの配布物は、次の4つを`share/doc/eml/`に入れる。`nix build`が置く。

- `LICENSE.txt`: emlのライセンス (MIT)
- `THIRD-PARTY-LICENSES.md`: バイナリに入る他者のクレートの一覧と全文。`about.hbs`が、クレートでないGUIのtoolkitを書く「System libraries」の節を足す。cargo-aboutが生成するので手では直さない ([アーキテクチャ](architecture.md) の「外部 crate」)
- `GPL-3.0.txt`と`LGPL-3.0.txt`: FSFの文面をそのまま置いたもの。リポジトリでは`licenses/`にある。LGPLv3はGNU GPLv3に許可を足す形で、4節のbが両方の全文を添えることを求める

誰が何を配るかは、次の3つの場合で決まる。

- 配布物がemlそのもの (Nixのパッケージとclosure) のとき: 上の4つのファイルを添える。closureをバイナリキャッシュとして公開する者は、Qtそのものを配る者になり、そのQtのソースはnixpkgsが公開しているものに頼る。Qtを同梱したtarballを配るときは、同梱したそのQtのソースへの案内を負う
- ネイティブの出力の作者が、利用者のシステムのQtにつなぐとき: Qtの通知、GPLとLGPLの全文、動的なリンクを守る。利用者がQtを差し替えられるので、Qtのソースを配る義務は生じない
- ネイティブの出力の作者が、Qtを同梱して配るとき: 上に加えて、同梱したQtのソースを配る義務を負う

## Windows App SDKのruntime

emlはMicrosoftのファイルを何も配らない。bootstrapのDLLも、`.winmd`も、インストーラも含めない。Windows App SDKのライセンスの条件をemlに持ち込まないためである。

- runtimeは利用者が入れる。bootstrapは`AddPackageDependency`で、パッケージのfamily `Microsoft.WindowsAppRuntime.2_8wekyb3d8bbwe` (`winio-winui3`の`V2`) を依存に足す。呼び出しはWindows 11のkernelbaseが持つ。プロセスで1回、プログラムが初めてGUIを求めたときだけ呼び、依存はプロセスの間生きる
- Windows 11 (build 22000以降) かどうかは、bootstrapの前に確かめる。足りなければ`WinUI needs Windows 11 or later`で失敗する。そのため、runtimeのDLLを名前だけで読み込む道には入らない
- runtimeがなければ、`open_ui_raw`は`NotAvailable`で失敗し、文言がMicrosoftの配布のページを示す ([標準ライブラリ](../spec/stdlib.md) の「`Std.Gui`」)
- runtimeを探す規則は、family名の後ろの2とバージョン0で組んである。入れたruntimeに実機で当たるかは確かめていない。当たらなければ、Microsoftの文書のfamily名を使い、最小のバージョンを2.0.1.0にする
- runtimeを含めて自己完結にする配り方 (self-contained) は、MSBuildの手順しか文書になく、使わない
- Windows App SDK 2のライセンスとインストーラの条件は、まだ読んでいない。emlは何も配らないので、emlの側に条件は生じない。プログラムの作者がインストーラを同梱して配るときは、Microsoftの条件の下で行うことになる

## macOSの署名と公証

`.app`のバンドルは作らず、裸の実行ファイルのままである。

- 組んだマシンで動かすだけなら、linkerが付けるad-hocの署名で足りる
- バイナリを配る作者は、Developer IDの証明書で署名し、hardened runtimeとsecure timestampを付け、公証 (notarization) を受ける。裸の実行ファイルをzipに入れて出すので、公証の票をバイナリに貼る (staple) ことはできず、Gatekeeperは公証をオンラインで確かめる
- `cranelift-jit`は`MAP_JIT`を使わないので、hardened runtimeで署名した`eml`は、Cの関数の橋渡しを作れない ([VM](../spec/vm.md) の「橋渡しの関数」)

## AOTの出力

`eml build`はまだない。AOTの出力に課す規則は [ネイティブ化の設計](../future/native.md) の「AOT と配布」にあり、ここには、その規則のうちライセンスと配る人の義務につながるものだけを挙げる。実装した振る舞いでなく、`eml build`を作るときに守る設計である。

- GUIを使うプログラムは`eml build --profile gui`で組む。プロファイルは`console` (既定) と`gui`で、何をリンクするかはプロファイルだけが決め、importからは推し量らない。`console`で`Std.Gui`をimportするプログラムを組むと、リンクの前に失敗する
- リンクするGUIのライブラリとCのruntimeは、既定で動的にする。Qt 6は利用者のシステムのQtで、上の「Qtを配る規則」の2つ目の場合になる。Windows App SDKはリンクの時に名指さず、実行のときにbootstrapで探す
- 出力を配るときは、emlが実行系の静的ライブラリと並べて配る通知のファイル (`THIRD-PARTY-LICENSES-runtime.md`) を、`eml build`が出力の隣にコピーする。出力を配り直すときは、このファイルも一緒に配る

## 他者のクレートのライセンス

GUIのために足したクレート (`taffy`、`slotmap`、`smallvec`、serdeの系統、`cxx`の系統、`foldhash`、`objc2`の系統、`windows-*`の0.100の系統、`winio-winui3`、`winio-windows-subset`) は、どれも`about.toml`の`accepted`にあるライセンスで使える。`accepted`は変えない。クレートを足したり上げたりしたら、`THIRD-PARTY-LICENSES.md`を`nix develop -c nix/third-party-licenses.sh`で書き直す。CIの`licenses`の手順が、コミットしたファイルと生成した結果を比べる ([アーキテクチャ](architecture.md) の「外部 crate」)。
