# 把 AERA 浏览器接入 Skyward Recovery

本文记录当前 `aera-browser` 分支把 AERA WPE 浏览器接进 Skyward 的方案、实现边界、构建方法和需要注意的地方。它不是 WebView 移植，而是把 AERA 的独立 WPE worker 当成一个受隔离的浏览器后端，由 Skyward 提供页面、输入、下载管理、键盘和显示。

当前目标设备是 Xiaomi 14 Ultra（`aurora`），Recovery 屏幕为 1440×3200。

---

## 1. 总体结构

```text
Skyward recovery (Rust)
  |
  |  Screen::AeraBrowser
  |  Aera state / actions / effects
  v
skyward-aera
  |  AERA protocol
  |  Session (memfd frame + SOCK_SEQPACKET control)
  |  Process (fork/exec jail)
  v
/system/bin/aera-browser-jail
  |  private mount namespace
  |  chroot /tmp/aera-web
  |  uid 99090, gid 99090, supplementary gid 3003
  |  capability drop, no_new_privs, seccomp
  v
/tmp/aera-web/usr/bin/aera-browser-worker
  |
  |  WPE WebKit
  |  Mesa zink -> Vulkan freedreno/Turnip -> KGSL
  v
1080x2100 ARGB frames -> sealed memfd -> Skyward
```

Skyward 永远不解析网页，也不把 recovery 的路径、文件描述符或渲染目标交给 web 内容。网页只和 WPE worker 交互；worker 只通过固定的 2072 字节协议和两块帧槽与 Skyward 通信。

---

## 2. 代码组成

### 2.1 `crates/skyward-aera`

AERA 控制面。

- `protocol.rs`
  - `MAGIC = 0x41525032`
  - `Message` 固定 2072 字节
  - `Kind`：Open/Back/Forward/Reload/Stop/Touch/Key/Ack/Close、Frame/Status/Error、KeyboardShow/Hide、Download 事件
  - `WIDTH=1080`、`HEIGHT=2100`、`VIEW_WIDTH=360`、`VIEW_HEIGHT=700`、`DEVICE_SCALE=3.0`
  - `FRAME_SLOTS=2`
  - 地址校验：只允许 `http://`/`https://`
- `session.rs`
  - 接管 memfd 和 `SOCK_SEQPACKET` 控制 socket
  - 检查 memfd 大小和 seal
  - `poll()` 读取最多 8 条消息
  - `frame()` 返回当前双槽帧
  - `acknowledge()` 让 worker 发布下一帧
  - `touch()`、`key()`、`open()`、`back()`、`forward()`、`reload()`
  - `keyboard()`、`take_downloads()`
- `process.rs`
  - 创建 sealed memfd
  - 创建 `SOCK_SEQPACKET` socketpair
  - `fork()` 后 `execv("/system/bin/aera-browser-jail", "--browser", runtime)`
  - 子进程使用 fd 3/4 固定描述符
  - 父进程持有 `Process`；`Drop` 时向进程组发 SIGTERM，再 SIGKILL
- `bin/aera-probe.rs`
  - 独立探针：启动 worker、打开 URL、等待 frame、保存 PPM，用来排除 Skyward UI

### 2.2 `crates/aera-browser-jail`

启动 worker 的隔离器。

职责：

- `unshare(CLONE_NEWNS)` 和 private mount propagation
- bind runtime，建立 `dev`/`proc`/`tmp`/`run`/`downloads`/`profile`/`etc`
- 绑定只读：
  - `/dev/kgsl-3d0`
  - `/dev/dma_heap/system`
  - `/dev/ion`
  - `/dev/null`、`/dev/zero`、`/dev/urandom`、`/dev/random`
- 创建 `/downloads` 并绑定 `/sdcard/AERA/Downloads`
- 写 `/etc/resolv.conf`（网关 + 223.5.5.5 + 1.1.1.1），并修正为 `0644`
- 绑定 `/skyward/MiSansVF.ttf` 为 `/usr/share/fonts/aera/MiSansVF.ttf`
- 写 fontconfig 规则，让 `MiSans VF` 成为 `sans-serif`
- 把 runtime root 改为只读
- `chroot`、`chdir("/")`
- `setgroups(3003)`、`setgid(99090)`、`setuid(99090)`
- 清空 capability bounding set
- `PR_SET_NO_NEW_PRIVS`
- seccomp filter（当前是 permissive 测试版）
- `PR_SET_DUMPABLE=0`
- exec worker，并传入固定环境（见第 4 节）

### 2.3 `crates/skyward-ui`

页面层。

- `aera.rs`
  - `browser()`：浏览器页
  - `address()`：兼容旧地址页（现在主要使用底部地址弹窗）
  - `browser_rect()`：计算 1080×2100 帧在屏幕上的 letterbox 区域
  - `view_point()`：屏幕坐标映射到 360×700
  - 底部双行工具栏：地址 + 图标按钮
  - 白色圆形返回按钮
  - 历史/书签/下载弹层
  - 透明页面模式：KMS overlay 可用时不再把帧画进 Canvas
- `app/aera.rs`
  - `Aera` 状态：地址、加载地址、frame、状态、是否为 GPU 合成、键盘、地址弹窗、历史、书签
  - `AeraPanel::{History, Bookmark, Downloads}`
- `app/mod.rs`
  - `Screen::AeraBrowser`
  - `Action::AeraOpen/AeraBack/AeraForward/AeraReload/AeraTap/AeraKey/AeraTouch/...`
  - `Effect::AeraStart/AeraOpen/AeraBack/AeraForward/AeraReload/AeraTap/AeraTouch/AeraKey`
- `app/input.rs`
  - 网页区域按下 → AERA `TouchDown`
  - 拖动 → `TouchMove`
  - 抬起 → `TouchUp`
  - 页面控件固定，左右边缘按下仍进入 Skyward 的返回手势
- `app/act.rs`
  - 键盘 Shift / 数字字母层切换
  - 地址弹窗输入
  - 文件型 URL 的侧路下载
  - 历史/书签/下载管理器动作

### 2.4 `crates/skyward`

运行时胶水。

- `ui.rs`
  - `Ui.aera: Option<Process>`
  - `aera_told()`：poll、ack、下载事件、键盘状态、状态栏
  - `start_side_download()`：AERA 没有发 DownloadStarted 时，按状态栏 URL 直接走下载管理器 worker
  - `ensure_aera_runtime()`：解压/更新 runtime
  - `show()`：把浏览器 frame/render rect 交给 Surface；GPU 层失败时回退 CPU
- `browser.rs`（旧浏览器 worker）
  - 下载管理器的自定义链接下载复用 `Worker::Task::Save`

---

## 3. 协议和帧管线

### 3.1 固定消息

```rust
#[repr(C)]
pub struct Message {
    pub magic: u32,
    pub kind: u32,
    pub sequence: u32,
    pub x: i32,
    pub y: i32,
    pub value: u32,
    pub text: [u8; 2048],
}
```

约束：

- `size_of::<Message>() == 2072`
- `align_of::<Message>() == 4`
- 不能增加字段
- 不能用 JSON/变长协议代替
- 修改协议必须同步修改 AERA worker

### 3.2 双帧槽

- memfd 大小为 `FRAME_BYTES * 2`
- worker 写 slot `sequence % 2`
- Skyward 读当前 slot
- `Ack` 后 worker 才发布下一帧
- Skyward 可以在 UI 忙时丢弃帧，但仍必须 ack，否则 worker 会停
- 帧字节序为 little-endian B,G,R,A

### 3.3 触摸

屏幕坐标不是 AERA 坐标。

```text
screen (x,y)
  -> browser_rect() letterbox
  -> 360x700 view
  -> Message TouchDown/Move/Up
```

注意：

- 只能触摸实际 frame 区域
- 工具栏、地址栏、键盘、状态栏留白不属于网页
- 页面按 12×20 网格提供 click 目标；拖动时使用连续映射
- 侧滑返回必须优先于网页触摸；否则手势会被网页吃掉

### 3.4 键盘

- worker 发 `KeyboardShow(purpose)`
- Skyward 画键盘，按键变成 `Kind::Key` 的 Unicode 值
- 退格 8、回车 13、空格 32
- Shift / `Cap::Show(layer)` 只改 Skyward 键盘层，不发给网页
- 地址弹窗的输入要写 `state.aera.address`，不能发给网页

---

## 4. worker 环境

jail 固定的环境（节选）：

```text
PATH=/usr/bin
HOME=/tmp
TMPDIR=/tmp
LANG=en.UTF-8
XDG_RUNTIME_DIR=/run
XDG_CACHE_HOME=/tmp/cache
GSETTINGS_BACKEND=memory
GIO_MODULE_DIR=/usr/lib/gio/modules
GIO_USE_PROXY_RESOLVER=dummy
GIO_USE_NETWORK_MONITOR=base
WEBKIT_FORCE_SANDBOX=0
VK_DRIVER_FILES=/usr/share/vulkan/icd.d/freedreno_icd.json
MESA_LOADER_DRIVER_OVERRIDE=zink
LIBGL_DRIVERS_PATH=/usr/lib/dri
EGL_PLATFORM=surfaceless
ZINK_DESCRIPTORS=lazy
ZINK_DEBUG=sync,flushsync
MESA_SHADER_CACHE_DIR=/tmp/cache/mesa
MESA_SHADER_CACHE_DISABLE=false
GST_PLUGIN_SYSTEM_PATH=/usr/lib/gstreamer-1.0
GST_PLUGIN_SCANNER=/usr/libexec/gstreamer-1.0/gst-plugin-scanner
WEBKIT_CAIRO_PAINTING_THREADS=6
ICU_DATA=/usr/share/icu/76.1
FONTCONFIG_PATH=/etc/fonts
```

要点：

- `GIO_USE_PROXY_RESOLVER=dummy` / `GIO_USE_NETWORK_MONITOR=base` 很重要。recovery 没有 desktop D-Bus/network manager，GIO 默认实现可能绕过 libc DNS，直接报 resolver failure。
- `EGL_PLATFORM=surfaceless` 让 WPE 在 recovery 下用无窗口 EGL。
- `VK_DRIVER_FILES` 指向 freedreno ICD。
- `/dev/kgsl-3d0` 不可用时，zink 会直接失败，页面不显示。
- 当前 runtime 还有 `libaera-gpu.so` 设置 shim，用 `webkit_settings_new_with_settings` 覆盖默认值，强制开启 WebGL 和 2D canvas acceleration。AERA worker 原始设置把 WebGL 关掉了。

---

## 5. 显示路径

### 5.1 默认 CPU 路径

```text
1080x2100 BGRA
  -> Skyward Canvas 缩放
  -> KMS primary framebuffer
```

优点：任何设备都能显示。  
缺点：每帧 1080×2100 → 屏幕的 CPU 缩放是主要卡顿来源。

### 5.2 KMS overlay 实验路径

当前分支尝试：

```text
1080x2100 浏览器帧 -> KMS primary plane
Skyward 控件（透明页面）-> KMS overlay plane
```

设计：

- AERA 页面背景透明
- 浏览器帧由显示处理器缩放到 `browser_rect()`
- 工具栏、返回按钮、状态栏保持不透明
- `alpha`/`blend_op`/`zpos` 由面板属性决定
- 失败时回退 CPU

已知问题：

- Aurora 面板的属性是 `alpha`、`blend_op`、`zpos`，没有 `pixel blend mode`
- overlay plane 不支持缩放浏览器帧；现在改为浏览器帧走 primary，控件走 overlay
- KMS overlay 很敏感；`flip: Invalid argument` 表示 plane 组合/格式/缩放不合法
- 没有在所有 recovery/KGSL 状态下验证
- 回退日志：
  ```text
  the browser GPU layer failed: ...
  ```

### 5.3 为什么不是“纯 GPU 合成”

即使 KMS overlay 生效，浏览器帧仍然由 WPE 的 GPU 渲染，然后：

- 写回 SHM
- 由 KMS/DPU 扫描输出

要彻底去掉 readback，需要 worker 输出 DMA-BUF，并让 KMS 直接导入。这不在当前 AERA worker 协议里，需要改 worker 和协议。

---

## 6. runtime 部署

AERA runtime 约 68 MB，不能放进 100 MB recovery 分区。

### 6.1 放置

```sh
adb push aera-runtime-fixed.tar.gz /sdcard/aera-runtime.tar.gz
```

### 6.2 首次打开自动解压

`ensure_aera_runtime()`：

1. 如果 `/tmp/aera-web/usr/bin/aera-browser-worker` 存在且 marker 与 tar 大小一致，直接使用
2. 否则删除 `/tmp/aera-web`
3. `mkdir -p /tmp/aera-web`
4. `/system/bin/tar -xzf /sdcard/aera-runtime.tar.gz -C /tmp/aera-web`
5. 写 `/skyward/run/aera-runtime.size`

`/tmp` 是 tmpfs，每次重启 recovery 都要重新解压。

### 6.3 手动解压

```sh
adb shell "rm -rf /tmp/aera-web && mkdir -p /tmp/aera-web && tar -xzf /sdcard/aera-runtime.tar.gz -C /tmp/aera-web"
```

### 6.4 runtime 内容

至少需要：

```text
usr/bin/aera-browser-worker
usr/lib/libWPEWebKit-2.0.so.1
usr/libexec/wpe-webkit-2.0/WPEWebProcess
usr/libexec/wpe-webkit-2.0/WPENetworkProcess
usr/lib/libEGL.so.1
usr/lib/libGLESv2.so.2
usr/lib/libvulkan.so.1
usr/lib/libvulkan_freedreno.so
usr/lib/dri/zink_dri.so
usr/lib/libgallium-*.so
usr/share/fonts/aera/...
usr/share/vulkan/icd.d/freedreno_icd.json
usr/lib/libaera-gpu.so        # 当前分支的 WebGL 设置 shim
```

检查软链：

```sh
ls -l /tmp/aera-web/usr/lib/libgallium*.so
readlink -f /tmp/aera-web/usr/lib/libgallium*.so
```

`libgallium` 必须指向 `dri/zink_dri.so`，不能是相对 `/usr/lib` 的错误路径。

---

## 7. Aurora 配置

`devices/aurora.toml` 需要：

- `[modules] load` 包含 `msm_kgsl`
- GPU service：
  ```sh
  chmod 666 /dev/kgsl-3d0 /dev/dma_heap/system
  ```
- 确保 `msm_kgsl.ko` 已加载
- 可选 GPU tuning：
  ```sh
  K=/sys/class/kgsl/kgsl-3d0
  echo 0 > $K/min_pwrlevel
  echo performance > $K/devfreq/governor
  echo 0 > $K/preemption
  echo 0 > $K/bus_split
  echo 0 > $K/ifpc
  ```
- firmware：
  - `gen70900_sqe.fw`
  - `gen70900_aqe.fw`
  - `gmu_gen70900.bin`
  这些必须能在 `/lib/firmware` 找到；否则 KGSL 打开失败或 AHB 错误。

注意：`preemption=0` / `ifpc=0` 并不是所有 KGSL 版本都接受；如果写入后浏览器打不开，先恢复默认。

---

## 8. 下载

有两条路径。

### 8.1 AERA 原生下载

worker 的 `DownloadDestination` 把文件写到 `/downloads/<name>`，绑定到：

```text
/sdcard/AERA/Downloads
```

worker 发送：

- `DownloadStarted`
- `DownloadProgress`
- `DownloadFinished`
- `DownloadFailed`
- `DownloadCancelled`

`Session::take_downloads()` 交给 `ManagerDownloads`，再进入独立下载管理器。

### 8.2 侧路下载

实测 coolapk 的 APK：

```text
status: https://dl-t2.coolapkmarket.com/...apk?...
status: Frame load interrupted
```

worker 没有发 `DownloadStarted`。因此 `Ui::aera_told()` 会检查状态 URL：

- 后缀 `.apk`
- `.apks`
- `.xapk`
- `.zip`
- `.7z`
- `.bin`
- `.img`

命中后由 Skyward 自己的下载 worker 拉取同一个 URL，写入下载管理器。这样不依赖 AERA worker 的 response policy。

注意：

- 这种侧路下载不带浏览器的 Cookie/Referer
- 如果 CDN 有防盗链，会失败
- URL 必须是状态栏里出现的最终地址
- 只能对文件后缀做启发式判断

### 8.3 独立下载管理器

高级 → 下载管理：

- 列表显示 AERA 下载和自定义链接
- 取消/重试/清理
- 自定义 `http://`/`https://` URL
- 下载目录：
  ```text
  /sdcard/AERA/Downloads
  ```
- 自定义链接使用 `crate::browser::Worker::Task::Save`
- 后台 worker 不依赖 AERA 页面可见

限制：

- 下载列表只在本次 recovery 运行中保留
- 重启后列表清空，文件保留
- 没有断点续传/暂停
- 没有逐项下载速度

---

## 9. 页面生命周期

- 进入 `Screen::AeraBrowser` 时 `Effect::AeraStart`
- 离开页面回 Home：
  - 保留 worker 2 秒宽限
  - 如果有 AERA 下载在运行，继续保留
  - 否则结束进程
- `Process::Drop` 向进程组发 SIGTERM，再 SIGKILL
- AERA 页面不可见时不再复制 frame

注意：AERA 页面和下载管理器共享一个 worker。关闭浏览器时如果还有 AERA 下载，进程会保留。

---

## 10. 构建和测试

### 10.1 编译

```sh
cargo build --offline --release -p skyward-aera --target aarch64-linux-android
cargo build --offline --release -p aera-browser-jail --target aarch64-linux-android
cargo build --offline --release -p skyward --target aarch64-linux-android
```

需要：

```text
ANDROID_NDK_HOME
CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER
CC_aarch64-linux-android
AR_aarch64-linux-android
RANLIB_aarch64-linux-android
```

### 10.2 单测

```sh
cargo test --offline -p skyward-ui
```

当前 339 个 UI 测试通过。

### 10.3 探针

```sh
adb shell "AERA_JAIL=/system/bin/aera-browser-jail /system/bin/aera-probe /tmp/aera-web /tmp/frame.ppm https://example.com"
```

期望：

```text
frame 1: ...
frame 2: ...
saved /tmp/frame.ppm
```

### 10.4 打包

```sh
skyward-pack put --partition-size 104857600 \
  --out aura.img base.img \
  system/bin/recovery target/aarch64-linux-android/release/skyward
```

同时要放：

```text
system/bin/aera-browser-jail
system/bin/aera-probe
skyward/device.toml
skyward/MiSansVF.ttf
```

---

## 11. 诊断

### 11.1 AERA 日志

```sh
adb shell "cat /skyward/run/aera.log"
```

应看到：

```text
AERA worker started
status: ...
open ...
side download ...
browser closed on returning home
```

### 11.2 内核

```sh
adb shell "dmesg | grep -E 'kgsl|AHB|firmware|gen70900'" 
```

常见：

```text
kgsl kgsl-3d0: CP: AHB bus error
CP_RL_ERROR_DETAILS_0:0x10008e07
```

这不是 UI 层错误，是 KGSL/GMU 电源/fence 竞争。

### 11.3 GPU

```sh
adb shell "cat /sys/class/kgsl/kgsl-3d0/gpu_busy_percentage"
adb shell "cat /sys/class/kgsl/kgsl-3d0/gpuclk"
adb shell "cat /proc/<worker-pid>/root/usr/lib/..."
```

### 11.4 KMS

```sh
adb shell "/system/bin/skyward kms --card /dev/dri/card0"
```

关注：

- primary plane 的 `alpha`、`blend_op`、`zpos`
- overlay plane 的 `zpos`
- overlay formats 是否包含 `Abgr8888`

如果 Skyward 打印：

```text
the browser GPU layer failed: flip: Invalid argument
```

表示 KMS plane 组合不合法，已回退 CPU。

### 11.5 DNS

```sh
adb shell "cat /tmp/aera-web/etc/resolv.conf"
```

文件必须对 uid 99090 可读：

```text
-rw-r--r-- root root
```

内容示例：

```text
nameserver 192.168.1.1
nameserver 223.5.5.5
nameserver 1.1.1.1
```

如果文件是 `0600 root`，worker 读不到 DNS，会表现为 `Error resolving ... Try again`。

---

## 12. 已知问题

1. **seccomp 是 permissive 测试版**
   - 在生产前必须换成 AERA 的完整 allowlist
   - 当前 allow-all 只用于 bring-up

2. **KGSL AHB bus error**
   - 周期性 `CP: AHB bus error`
   - 这不是浏览器 UI 能修的
   - 需要内核/GMU/firmware 层处理

3. **runtime 不在镜像里**
   - `/sdcard/aera-runtime.tar.gz` 必须存在
   - `/tmp` 重启丢失，需要重新解压

4. **UA/WebGL 是 runtime 二进制补丁**
   - UA 在 worker 里是硬编码字符串
   - WebGL 通过 `libaera-gpu.so` 覆盖设置
   - 换 worker 版本后需要重新打补丁

5. **下载 response policy**
   - AERA worker 对 `NEW_WINDOW_ACTION` 直接 ignore
   - 部分站点不会产生 `DownloadStarted`
   - 当前靠状态 URL 后缀做侧路下载
   - 有防盗链/Cookie/Referer 的站点可能失败

6. **CPU 帧缩放**
   - 1080×2100 每帧缩放到屏幕仍然昂贵
   - KMS overlay 是实验路径，未在所有设备状态验证
   - 失败会回退 CPU

7. **进程生命周期**
   - 返回 Home 后 2 秒关闭 worker
   - 有 AERA 下载时保留
   - 下载管理器列表不持久化

8. **键盘**
   - 只刷新键盘区域和 frame 相交 band
   - 如果 KMS overlay 不可用，打字仍可能触发 CPU frame 重绘

9. **DNS**
   - `/etc/resolv.conf` 必须 0644
   - jail 会优先写网关，再写公共 DNS
   - 1.1.1.1 在部分网络不可达，不应作为唯一 DNS

10. **字体**
    - MiSans 必须绑定到 runtime 的 font path
    - fontconfig 的 `local.conf`/`conf.d` 文件必须是 uid 99090 可读

---

## 13. 新设备移植清单

1. 确认设备有可用的 WPE runtime（musl，aarch64）
2. 确认 KGSL/GPU 在 recovery 下可加载
3. 确认 GPU firmware 可用
4. 确认 `/dev/kgsl-3d0`、`/dev/dma_heap/system` 可访问
5. 确认面板分辨率/拓扑（mixer 数量、DSC）
6. 修改 `aera.rs` 的 `browser_rect` 和工具栏尺寸
7. 修改 `WIDTH/HEIGHT/VIEW_WIDTH/VIEW_HEIGHT/DEVICE_SCALE`
8. 修改 jail 的 runtime 路径、uid、下载路径
9. 确认 `/sdcard` 在解密后可用
10. 写 DNS/MiSans 绑定
11. 用 `aera-probe` 先验证 frame
12. 再接 UI
13. 再做 KMS/GPU 实验路径

---

## 14. 结论

AERA 接入 Skyward 的核心不是“把浏览器嵌进 Rust UI”，而是：

- 保持 AERA worker 独立
- 用固定协议和 memfd 交换帧
- Skyward 只做控制器、输入映射、键盘和下载管理
- 显示层先走 CPU，再实验 KMS overlay
- 所有失败路径都要能回退

当前可工作的最小闭环是：

```text
AERA frame -> Skyward Canvas -> KMS scanout
```

KMS overlay/GPU 合成是实验增强，不是稳定基线。上线前必须优先完成：

- 完整 seccomp allowlist
- 下载 response policy
- KGSL AHB 错误处理
- runtime 版本/校验
- KMS overlay 的实机回归
