#!/usr/bin/env python3
"""AgentCore-Light（杰理 AC696N）协议测试/演示工具。

USB 串口部分只用标准库（不依赖 pyserial）；BLE 需要 bleak。

四种连接方式：

1. 直连 USB 串口（需要厂商后台服务已停止，否则会抢读）：

       python3 scripts/agentcore_probe.py -p /dev/ttyACM0 status

2. 走厂商服务的 HTTP 透传（服务在跑时用这个，设备输出从服务日志里跟踪）：

       python3 scripts/agentcore_probe.py --service demo

3. 蓝牙 BLE（Nordic UART Service，连上不发 HELLO）：

       python3 scripts/agentcore_probe.py --ble status
       python3 scripts/agentcore_probe.py --ble --address AA:BB:CC:DD:EE:FF state BUSY

   注意：设备插着 USB 时不广播（STATUS 里 BLE_ADV:0），要先拔掉 USB。

4. 蓝牙 SPP（AF_BLUETOOTH socket，免 root；需要先配对，协议上要 HELLO）：

       /usr/bin/python3 scripts/agentcore_probe.py --spp AA:BB:CC:DD:EE:FF status
       /usr/bin/python3 scripts/agentcore_probe.py --spp AA:BB:CC:DD:EE:FF:2 demo

   注意：uv/静态构建的解释器可能没有 socket.AF_BLUETOOTH，用系统 python3。
   想把它当普通串口用（/dev/rfcomm0）才需要 root：--spp-bind 或 sudo rfcomm bind。

协议摘要（详见 docs/AgentCore-Light-serial-protocol.md）：

    115200 8N1, DTR=1/RTS=0, 行结尾 '\\n', 纯 ASCII/UTF-8

    HELLO                  -> HELLO:OK
    STATUS                 -> STATUS:IDLE,FW:v0.5.13-jl-ota,BRIGHTNESS_LEVEL:30,...
    GET_SETTINGS           -> SETTINGS:...,STORE:VM  +  SLEEP_SETTINGS:...
    GET_BT_NAME            -> BT_NAME:CURRENT:AgentCore-Light
    IDLE/BUSY/SUCCESS/...  -> STATE:<state>  (+ STATUS 推送)
    BRIGHTNESS_LEVEL:0-300 -> STATUS 推送
    BUZZER:ON|OFF, BUZZER_VOLUME:0-100, LOW_BATTERY_ALERT:0|1
    SOUND:<NAME>:once|loop -> SOUND:<NAME>
    IDLE_SLEEP_SEC:<s>, BT_DISCONNECT_OFF_SEC:<s>
    设备每 5s 主动上报 CHG:<0|1>,VBAT:<0.01V>,PER:<%>,CUR:<mA>
"""

from __future__ import annotations

import argparse
import asyncio
import errno
import fcntl
import glob
import json
import os
import re
import select
import socket
import struct
import sys
import termios
import threading
import time
import urllib.error
import urllib.request

BAUDRATE = termios.B115200
TIOCM_DTR = 0x002
TIOCM_RTS = 0x004
SERVICE_URL = "http://127.0.0.1:18792"
DEFAULT_LOG = os.path.expanduser("~/.local/state/AgentCoreLight/logs/serial_service.log")

STATES = ("IDLE", "BUSY", "SUCCESS", "ERROR", "WAIT_CONFIRM", "CONFIRM", "WAITING", "OFF")


# --------------------------------------------------------------------------- #
# 颜色
# --------------------------------------------------------------------------- #
class C:
    on = sys.stdout.isatty()

    @staticmethod
    def wrap(code: str, text: str) -> str:
        return f"\033[{code}m{text}\033[0m" if C.on else text

    @classmethod
    def dim(cls, t):
        return cls.wrap("2", t)

    @classmethod
    def cyan(cls, t):
        return cls.wrap("36", t)

    @classmethod
    def green(cls, t):
        return cls.wrap("32", t)

    @classmethod
    def red(cls, t):
        return cls.wrap("31", t)

    @classmethod
    def yellow(cls, t):
        return cls.wrap("33", t)


# --------------------------------------------------------------------------- #
# 直连串口
# --------------------------------------------------------------------------- #
class SerialLink:
    """最小 115200 8N1 raw 串口，带后台读线程。"""

    def __init__(self, port: str):
        self.port = port
        fd = os.open(port, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
        self.fd = fd

        attrs = termios.tcgetattr(fd)
        attrs[0] &= ~(
            termios.IGNBRK
            | termios.BRKINT
            | termios.PARMRK
            | termios.ISTRIP
            | termios.INLCR
            | termios.IGNCR
            | termios.ICRNL
            | termios.IXON
            | termios.IXOFF
            | termios.IXANY
        )
        attrs[1] &= ~termios.OPOST
        attrs[2] &= ~(termios.CSIZE | termios.PARENB | termios.CSTOPB)
        attrs[2] |= termios.CS8 | termios.CREAD | termios.CLOCAL
        attrs[3] &= ~(termios.ECHO | termios.ECHONL | termios.ICANON | termios.ISIG | termios.IEXTEN)
        attrs[4] = attrs[5] = BAUDRATE
        attrs[6][termios.VMIN] = 0
        attrs[6][termios.VTIME] = 0
        termios.tcsetattr(fd, termios.TCSANOW, attrs)
        termios.tcflush(fd, termios.TCIOFLUSH)

        # 与厂商程序一致：DTR=1, RTS=0
        try:
            fcntl.ioctl(fd, termios.TIOCMBIS, struct.pack("i", TIOCM_DTR))
            fcntl.ioctl(fd, termios.TIOCMBIC, struct.pack("i", TIOCM_RTS))
        except OSError:
            pass
        time.sleep(1.5)  # POST_CONNECT_SETTLE_SECONDS
        termios.tcflush(fd, termios.TCIFLUSH)

        self._lines: list[tuple[float, str]] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._buf = ""
        self._echo = False
        self._thread = threading.Thread(target=self._reader, daemon=True)
        self._thread.start()

    # 内部：读线程 ---------------------------------------------------------
    def _reader(self) -> None:
        while not self._stop.is_set():
            try:
                ready, _, _ = select.select([self.fd], [], [], 0.1)
            except (OSError, ValueError):
                return
            if not ready:
                continue
            try:
                chunk = os.read(self.fd, 4096)
            except OSError as exc:
                if exc.errno in (errno.EIO, errno.ENODEV, errno.EBADF):
                    return
                continue
            if not chunk:
                continue
            self._buf += chunk.decode("utf-8", errors="ignore")
            *complete, self._buf = self._buf.split("\n")
            for raw in complete:
                line = raw.rstrip("\r").strip()
                if not line:
                    continue
                now = time.monotonic()
                with self._lock:
                    self._lines.append((now, line))
                if self._echo:
                    print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.green('←')} {line}")

    # 对外 ----------------------------------------------------------------
    def send(self, line: str) -> None:
        if self._echo:
            print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.cyan('→')} {line}")
        os.write(self.fd, (line + "\n").encode("utf-8"))
        termios.tcdrain(self.fd)

    def lines(self) -> list[tuple[float, str]]:
        with self._lock:
            return list(self._lines)

    def drain(self) -> None:
        with self._lock:
            self._lines.clear()

    def close(self) -> None:
        self._stop.set()
        try:
            self._thread.join(0.3)
        except RuntimeError:
            pass
        try:
            os.close(self.fd)
        except OSError:
            pass


# --------------------------------------------------------------------------- #
# 通过厂商服务
# --------------------------------------------------------------------------- #
class ServiceLink:
    """HTTP 透传到厂商本地服务；设备输出改从服务日志跟踪。"""

    def __init__(self, url: str = SERVICE_URL, log_path: str = DEFAULT_LOG):
        self.url = url.rstrip("/")
        self.log_path = log_path
        self._echo = False
        self._stop = threading.Event()
        self._lines: list[tuple[float, str]] = []
        self._lock = threading.Lock()
        self._offset = 0
        if os.path.exists(log_path):
            self._offset = os.path.getsize(log_path)
        self._thread = threading.Thread(target=self._tail, daemon=True)
        self._thread.start()

    def _tail(self) -> None:
        while not self._stop.is_set():
            try:
                with open(self.log_path, "r", encoding="utf-8", errors="ignore") as fh:
                    fh.seek(self._offset)
                    data = fh.read()
                    self._offset = fh.tell()
            except OSError:
                data = ""
            for raw in data.splitlines():
                m = re.search(r"\[DEVICE\] (.*)$", raw)
                if not m:
                    continue
                line = m.group(1).strip()
                with self._lock:
                    self._lines.append((time.monotonic(), line))
                if self._echo:
                    print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.green('←')} {line}")
            time.sleep(0.1)

    def send(self, line: str) -> None:
        if self._echo:
            print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.cyan('→')} {line}")
        body = json.dumps({"command": line}).encode()
        req = urllib.request.Request(
            f"{self.url}/command", data=body, headers={"Content-Type": "application/json"}, method="POST"
        )
        with urllib.request.urlopen(req, timeout=3) as resp:
            resp.read()
        time.sleep(0.12)  # 服务写入是 100ms 轮询

    def health(self) -> dict:
        with urllib.request.urlopen(f"{self.url}/health", timeout=2) as resp:
            return json.loads(resp.read().decode())

    def lines(self):
        with self._lock:
            return list(self._lines)

    def drain(self):
        with self._lock:
            self._lines.clear()

    def close(self):
        self._stop.set()
        self._thread.join(0.3)


# --------------------------------------------------------------------------- #
# BLE（Nordic UART Service）
# --------------------------------------------------------------------------- #
BLE_SERVICE_UUID = "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
BLE_RX_UUID = "6e400002-b5a3-f393-e0a9-e50e24dcca9e"  # 主机 → 设备（Write）
BLE_TX_UUID = "6e400003-b5a3-f393-e0a9-e50e24dcca9e"  # 设备 → 主机（Notify）


class BleLink:
    """用 bleak 连设备的 NUS 服务；行协议与串口一致，但不发 HELLO。"""

    def __init__(
        self,
        address: str | None = None,
        name_prefix: str = "AgentCore-Light",
        scan_timeout: float = 10.0,
    ):
        try:
            from bleak import BleakClient, BleakScanner
        except ImportError as exc:  # pragma: no cover
            raise SystemExit(
                "BLE 模式需要 bleak：pip install bleak\n" f"（{exc}）"
            ) from exc
        self._BleakClient = BleakClient
        self._BleakScanner = BleakScanner
        self.address = (address or "").strip()
        self.name_prefix = name_prefix
        self.device_name = ""
        self._echo = False
        self._lines: list[tuple[float, str]] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._buf = ""
        self._client = None
        self._loop = asyncio.new_event_loop()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()
        fut = asyncio.run_coroutine_threadsafe(self._connect(scan_timeout), self._loop)
        try:
            fut.result(scan_timeout + 20)
        except Exception:
            self.close()
            raise

    # 后台事件循环 --------------------------------------------------------
    def _run(self) -> None:
        asyncio.set_event_loop(self._loop)
        self._loop.run_forever()

    async def _connect(self, scan_timeout: float) -> None:
        if not self.address:
            print(C.dim(f"扫描 BLE 设备（前缀 {self.name_prefix}，{scan_timeout:.0f}s）…"))
            found = await self._BleakScanner.discover(timeout=scan_timeout, return_adv=True)
            candidates = []
            for addr, (dev, adv) in found.items():
                name = dev.name or adv.local_name or ""
                if name.startswith(self.name_prefix) and BLE_SERVICE_UUID in (adv.service_uuids or []):
                    candidates.append((addr, name, adv.rssi))
                elif name.startswith(self.name_prefix):
                    candidates.append((addr, name, adv.rssi))
            if not candidates:
                raise RuntimeError(
                    f"没扫到 {self.name_prefix}* 设备。\n"
                    "  常见原因：设备插着 USB —— 固件在 USB 模式下不开广播"
                    "（STATUS 里 BLE_ADV:0）。\n"
                    "  拔掉 USB 再试，或用 --address 指定已配对/缓存的地址。"
                )
            candidates.sort(key=lambda item: item[2], reverse=True)
            self.address, name, rssi = candidates[0]
            print(C.dim(f"选中 {name} ({self.address}) rssi={rssi}"))
        client = self._BleakClient(self.address, timeout=10.0)
        await client.connect()
        if not client.is_connected:
            raise RuntimeError("BLE 连接失败")
        self._client = client
        self.device_name = self.address
        await client.start_notify(BLE_TX_UUID, self._on_notify)

    def _on_notify(self, _sender, data: bytearray) -> None:
        self._buf += bytes(data).decode("utf-8", errors="ignore")
        *complete, self._buf = self._buf.split("\n")
        for raw in complete:
            line = raw.rstrip("\r").strip()
            if not line:
                continue
            with self._lock:
                self._lines.append((time.monotonic(), line))
            if self._echo:
                print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.green('←')} {line}")

    # 对外接口（与 SerialLink/ServiceLink 一致）---------------------------
    def send(self, line: str) -> None:
        if self._echo:
            print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.cyan('→')} {line}")
        payload = (line + "\n").encode("utf-8")

        async def _write() -> None:
            try:
                await self._client.write_gatt_char(BLE_RX_UUID, payload, response=False)
            except Exception:
                await self._client.write_gatt_char(BLE_RX_UUID, payload, response=True)

        asyncio.run_coroutine_threadsafe(_write(), self._loop).result(5)

    def lines(self):
        with self._lock:
            return list(self._lines)

    def drain(self):
        with self._lock:
            self._lines.clear()

    def close(self) -> None:
        self._stop.set()

        async def _bye() -> None:
            if self._client is not None:
                try:
                    await self._client.stop_notify(BLE_TX_UUID)
                except Exception:
                    pass
                try:
                    await self._client.disconnect()
                except Exception:
                    pass

        if self._loop.is_running():
            try:
                asyncio.run_coroutine_threadsafe(_bye(), self._loop).result(5)
            except Exception:
                pass
            self._loop.call_soon_threadsafe(self._loop.stop)
        self._thread.join(0.5)


# --------------------------------------------------------------------------- #
# 设备封装
# --------------------------------------------------------------------------- #
class Device:
    def __init__(self, link, skip_hello: bool = False):
        self.link = link
        self.skip_hello = skip_hello
        self.info: dict[str, str] = {}
        # 游标：每次等待只看调用之后新增的行，避免重复命中旧行
        self._cursor = 0

    # 基础 ----------------------------------------------------------------
    def _new_lines(self) -> list[str]:
        """取出并消费游标之后的新行（调用后游标推进）。"""
        all_lines = [line for _, line in self.link.lines()]
        if self._cursor > len(all_lines):  # 被 drain() 清空过
            self._cursor = 0
        fresh = all_lines[self._cursor :]
        self._cursor = len(all_lines)
        return fresh

    def wait_new(self, predicate, timeout: float = 1.5) -> str | None:
        self._new_lines()  # 把游标推到当前末尾
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            for line in self._new_lines():
                if predicate(line):
                    return line
            time.sleep(0.02)
        return None

    def collect_new(self, predicate, timeout: float = 1.5, quiet: float = 0.25) -> list[str]:
        """收集新增的匹配行；收到后静默 quiet 秒即认为一批结束。"""
        self._new_lines()
        got: list[str] = []
        deadline = time.monotonic() + timeout
        last_new = time.monotonic()
        while time.monotonic() < deadline:
            fresh = self._new_lines()
            if fresh:
                last_new = time.monotonic()
                got.extend(line for line in fresh if predicate(line))
            elif got and time.monotonic() - last_new >= quiet:
                break
            time.sleep(0.02)
        return got

    def send(self, line: str, expect: str | None = None, timeout: float = 1.5) -> str | None:
        if expect is None:
            self.link.send(line)
            return None
        self._new_lines()  # 游标推到当前末尾
        self.link.send(line)
        return self.wait_new(
            lambda s: s.startswith(expect) or s.startswith("ERR"), timeout
        )

    def handshake(self) -> bool:
        if self.skip_hello:
            print(C.dim("跳过 HELLO（BLE 链路不发握手，或链路已由厂商服务建立）"))
            return True
        self._new_lines()
        self.link.send("HELLO")
        line = self.wait_new(lambda s: s.startswith("HELLO"), 1.5)
        ok = line == "HELLO:OK"
        print(f"HELLO  {'→ ' + C.green('HELLO:OK') if ok else C.red('无应答')}")
        return ok

    def status(self) -> dict[str, str]:
        self._new_lines()
        self.link.send("STATUS")
        line = self.wait_new(lambda s: s.startswith("STATUS:"), 1.5)
        if not line:
            print(C.red("STATUS 无应答"))
            return {}
        body = line[len("STATUS:") :]
        fields: dict[str, str] = {}
        state = None
        for part in body.split(","):
            key, sep, value = part.partition(":")
            if not sep:
                state = state or part.strip()
                continue
            fields[key.strip().upper()] = value.strip()
        if state:
            fields["STATE"] = state
        self.info.update(fields)
        pretty = "  ".join(f"{k}={v}" for k, v in fields.items())
        print(f"STATUS  {pretty}")
        return fields

    def settings(self) -> dict[str, str]:
        self._new_lines()
        self.link.send("GET_SETTINGS")
        result: dict[str, str] = {}
        # SETTINGS 与 SLEEP_SETTINGS 在蓝牙链路上可能隔 1s 才到，等到两行都收到为止
        wanted = ("SETTINGS:", "SLEEP_SETTINGS:")
        got: list[str] = []
        deadline = time.monotonic() + 2.5
        while time.monotonic() < deadline:
            got.extend(line for line in self._new_lines() if line.startswith(wanted))
            if any(x.startswith("SETTINGS:") for x in got) and any(
                x.startswith("SLEEP_SETTINGS:") for x in got
            ):
                break
            time.sleep(0.03)
        for line in got:
            head, _, body = line.partition(":")
            for part in body.split(","):
                key, sep, value = part.partition(":")
                if not sep:
                    continue
                if key.strip().upper() == "STORE":
                    continue
                result[key.strip().upper()] = value.strip()
            print(f"{head:<15}{body}")
        if not result:
            print(C.red("GET_SETTINGS 无应答"))
        elif "IDLE_SLEEP_SEC" not in result:
            print(
                C.dim(
                    "（SLEEP_SETTINGS 不是 GET_SETTINGS 的应答：只在设置类命令后 ~1s 推送，"
                    "用 `sleep <秒> <秒>` 可读/写）"
                )
            )
        self.info.update(result)
        return result


# --------------------------------------------------------------------------- #
# 动作
# --------------------------------------------------------------------------- #
def cmd_state(dev: Device, name: str) -> None:
    name = name.upper()
    if name not in STATES:
        raise SystemExit(f"未知状态 {name}，可用：{', '.join(STATES)}")
    got = dev.send(name, "STATE:")
    print(f"{name:<13}{C.green(got) if got and got.startswith('STATE') else C.red(str(got))}")


def cmd_brightness(dev: Device, percent: int) -> None:
    level = max(0, min(300, int(percent) * 3))
    got = dev.send(f"BRIGHTNESS_LEVEL:{level}", "STATUS:")
    ok = got is not None
    print(f"{'brightness':<13}{percent}%  (level {level})  {C.green('ok') if ok else C.red('NO STATUS')}")


def cmd_buzzer(dev: Device, enabled: str, volume: int | None = None) -> None:
    if volume is not None:
        dev.send(f"BUZZER_VOLUME:{int(volume)}", "STATUS:")
        print(f"{'volume':<13}{volume}%")
    got = dev.send("BUZZER:ON" if enabled.lower() in ("on", "1", "true") else "BUZZER:OFF", "STATUS:")
    print(f"{'buzzer':<13}{enabled.upper():<4}{C.green('ok') if got else C.red('NO STATUS')}")


def cmd_lowbat(dev: Device, enabled: str) -> None:
    value = 1 if enabled.lower() in ("on", "1", "true") else 0
    got = dev.send(f"LOW_BATTERY_ALERT:{value}", "STATUS:")
    print(f"{'lowbat':<13}{value}  {C.green('ok') if got else C.red('NO STATUS')}")


def cmd_sound(dev: Device, name: str, mode: str = "once") -> None:
    got = dev.send(f"SOUND:{name.upper()}:{mode}", "SOUND:")
    print(f"{'sound':<13}{name.upper()}:{mode}  {C.green(got) if got else C.red('无应答')}")


def cmd_sleep(dev: Device, idle: int | None, disconnect: int | None) -> None:
    for line, label in (
        (f"IDLE_SLEEP_SEC:{idle}" if idle is not None else None, "idle sleep"),
        (f"BT_DISCONNECT_OFF_SEC:{disconnect}" if disconnect is not None else None, "bt off"),
    ):
        if not line:
            continue
        got = dev.send(line, "SLEEP_SETTINGS:")
        print(f"{label:<13}{line}  {C.green('ok') if got else C.red('NO SLEEP_SETTINGS')}")


def cmd_name(dev: Device, name: str) -> None:
    got = dev.send(f"SET_BT_NAME:{name}", "BT_NAME:") or "无应答"
    print(f"{'rename':<13}{name}  {C.green(got) if got.startswith('BT_NAME:OK') else C.red(str(got))}")


def cmd_raw(dev: Device, line: str) -> None:
    print(f"{'raw':<13}{line}")
    dev._new_lines()
    dev.link.send(line)
    for out in dev.collect_new(lambda s: True, 0.9, 0.22):
        print(f"{'':<13}{C.green('←')} {out}")


# --------------------------------------------------------------------------- #
# demo
# --------------------------------------------------------------------------- #
def demo(dev: Device, quick: bool = False) -> None:
    pace = 0.5 if quick else 1.1
    print(C.dim("\n== 1. 握手 / 自检 =="))
    dev.handshake()
    st = dev.status()
    cfg = dev.settings()

    original_level = int(st.get("BRIGHTNESS_LEVEL", 30) or 30)
    original_buzzer = st.get("BUZZER", "ON")
    original_volume = int(st.get("BUZZER_VOLUME", 10) or 10)
    original_lowbat = cfg.get("LOW_BATTERY_ALERT", "1")
    original_sleep = cfg.get("IDLE_SLEEP_SEC", "600")
    original_bt_off = cfg.get("BT_DISCONNECT_OFF_SEC", "300")

    try:
        print(C.dim("\n== 2. 灯效状态遍历（每档 1s）=="))
        for state in ("BUSY", "WAIT_CONFIRM", "SUCCESS", "ERROR", "IDLE"):
            cmd_state(dev, state)
            time.sleep(pace)

        print(C.dim("\n== 3. 音效（DONE / CLASSIC / ALERT）=="))
        for sound in ("DONE", "CLASSIC", "ALERT"):
            cmd_sound(dev, sound)
            time.sleep(pace + 0.4)

        print(C.dim("\n== 4. 亮度扫描 0→100%→10% =="))
        for percent in (0, 20, 40, 60, 80, 100):
            cmd_brightness(dev, percent)
            time.sleep(0.45)
        time.sleep(0.2)
        for percent in (70, 40, 10):
            cmd_brightness(dev, percent)
            time.sleep(0.45)

        print(C.dim("\n== 5. 蜂鸣器音量 10→50→10，开/关 =="))
        cmd_buzzer(dev, "on", 50)
        time.sleep(0.3)
        cmd_sound(dev, "POP")
        time.sleep(pace)
        cmd_buzzer(dev, "on", 10)
        time.sleep(0.3)
        cmd_buzzer(dev, "off")
        time.sleep(0.4)
        cmd_buzzer(dev, original_buzzer, original_volume)

        print(C.dim("\n== 6. 低电量提示开关 =="))
        cmd_lowbat(dev, "off")
        time.sleep(0.4)
        cmd_lowbat(dev, "on")
        time.sleep(0.4)

        print(C.dim("\n== 7. 未知命令 -> 错误格式 =="))
        cmd_raw(dev, "HELP")
        cmd_raw(dev, "BRIGHTNESS_LEVEL:999")

        print(C.dim("\n== 8. 看一条电量心跳 =="))
        line = dev.wait_new(lambda s: s.startswith("CHG:"), 7.0)
        print(f"{'CHG':<13}{C.green(line) if line else C.red('7s 内没有心跳')}")
    finally:
        print(C.dim("\n== 9. 还原初始设置 =="))
        dev.send(f"BRIGHTNESS_LEVEL:{original_level}", "STATUS:")
        print(f"{'brightness':<13}level {original_level}")
        dev.send("BUZZER:ON" if original_buzzer.upper() == "ON" else "BUZZER:OFF", "STATUS:")
        dev.send(f"BUZZER_VOLUME:{original_volume}", "STATUS:")
        print(f"{'buzzer':<13}{original_buzzer} / vol {original_volume}")
        dev.send(f"LOW_BATTERY_ALERT:{original_lowbat}", "STATUS:")
        print(f"{'lowbat':<13}{original_lowbat}")
        dev.send(f"IDLE_SLEEP_SEC:{original_sleep}", "SLEEP_SETTINGS:")
        dev.send(f"BT_DISCONNECT_OFF_SEC:{original_bt_off}", "SLEEP_SETTINGS:")
        print(f"{'sleep':<13}idle {original_sleep}s / bt-off {original_bt_off}s")
        cmd_state(dev, "IDLE")
        print(C.green("完成。"))


# --------------------------------------------------------------------------- #
# monitor
# --------------------------------------------------------------------------- #
def monitor(dev: Device, seconds: float) -> None:
    print(C.dim(f"监听设备输出 {seconds:.0f}s（Ctrl-C 结束）…"))
    dev.link._echo = True
    try:
        time.sleep(seconds)
    except KeyboardInterrupt:
        pass
    finally:
        dev.link._echo = False


# --------------------------------------------------------------------------- #
def find_port(explicit: str | None) -> str:
    if explicit:
        return explicit
    for pattern in ("/dev/ttyACM*", "/dev/ttyUSB*", "/dev/rfcomm*"):
        ports = sorted(glob.glob(pattern))
        if ports:
            return ports[0]
    raise SystemExit(
        "找不到串口：USB 插好？蓝牙 SPP 需要先绑 rfcomm（见 --help），或用 --ble。"
    )


class RfcommLink:
    """用 AF_BLUETOOTH socket 直连 SPP：不需要 root，也不需要 /dev/rfcomm*。

    与 rfcomm bind 的区别：bind 会让内核创建一个全局 TTY 设备（CAP_NET_ADMIN），
    这里只是普通 socket connect，权限模型和 TCP 一样。
    """

    def __init__(self, address: str, channel: int | None = None, scan_range=range(1, 16)):
        if not hasattr(socket, "AF_BLUETOOTH"):
            raise SystemExit(
                "当前解释器的 socket 模块不支持蓝牙（uv/静态构建常见）。\n"
                "  用系统解释器运行：/usr/bin/python3 scripts/agentcore_probe.py --spp ..."
            )
        self.address = address
        self.channel = channel
        self._echo = False
        self._lines: list[tuple[float, str]] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._buf = ""
        self.sock = None

        channels = [channel] if channel else list(scan_range)
        errors = {}
        for ch in channels:
            sock = socket.socket(socket.AF_BLUETOOTH, socket.SOCK_STREAM, socket.BTPROTO_RFCOMM)
            sock.settimeout(4.0)
            try:
                sock.connect((address, ch))
            except OSError as exc:
                errors[ch] = exc
                sock.close()
                continue
            self.sock = sock
            self.channel = ch
            break
        if self.sock is None:
            detail = "\n".join(f"  通道 {ch}: {exc}" for ch, exc in errors.items())
            raise SystemExit(
                f"SPP 连接失败（{address}）。设备配对好了吗？\n{detail}\n"
                f"  先用 bluetoothctl pair {address} 配对一次。"
            )
        self.sock.settimeout(0.2)
        self._thread = threading.Thread(target=self._reader, daemon=True)
        self._thread.start()

    def _reader(self) -> None:
        while not self._stop.is_set():
            try:
                ready, _, _ = select.select([self.sock], [], [], 0.1)
            except (OSError, ValueError):
                return
            if not ready:
                continue
            try:
                chunk = self.sock.recv(4096)
            except socket.timeout:
                continue
            except OSError:
                return
            if not chunk:
                return
            self._buf += chunk.decode("utf-8", errors="ignore")
            *complete, self._buf = self._buf.split("\n")
            for raw in complete:
                line = raw.rstrip("\r").strip()
                if not line:
                    continue
                with self._lock:
                    self._lines.append((time.monotonic(), line))
                if self._echo:
                    print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.green('←')} {line}")

    def send(self, line: str) -> None:
        if self._echo:
            print(f"  {C.dim(time.strftime('%H:%M:%S'))} {C.cyan('→')} {line}")
        self.sock.sendall((line + "\n").encode("utf-8"))

    def lines(self):
        with self._lock:
            return list(self._lines)

    def drain(self):
        with self._lock:
            self._lines.clear()

    def close(self) -> None:
        self._stop.set()
        try:
            self.sock.shutdown(socket.SHUT_RDWR)
        except OSError:
            pass
        try:
            self.sock.close()
        except OSError:
            pass
        self._thread.join(0.3)


MAC_RE = re.compile(r"^(?:[0-9A-Fa-f]{2}:){5}[0-9A-Fa-f]{2}$")


def parse_spp_spec(spec: str) -> tuple[str, int | None]:
    """接受 `AA:BB:CC:DD:EE:FF` 或 `AA:BB:CC:DD:EE:FF:通道`。"""
    text = spec.strip()
    if MAC_RE.match(text):
        return text.upper(), None
    mac, _, channel = text.rpartition(":")
    if MAC_RE.match(mac) and channel.isdigit():
        return mac.upper(), int(channel)
    raise SystemExit(f"SPP 地址格式错误：{spec}（应为 AA:BB:CC:DD:EE:FF[:通道]）")


def spp_bind(spec: str) -> str:
    """（可选）用 root 把 SPP 绑成 /dev/rfcommN —— 仅当你想把它当普通串口用时。"""
    import subprocess

    mac, channel = parse_spp_spec(spec)
    channel = str(channel or 2)
    existing = sorted(glob.glob("/dev/rfcomm*"))
    if existing:
        print(C.dim(f"复用已绑定的 SPP 端口 {existing[0]}"))
        return existing[0]
    index = "0"
    cmd = ["rfcomm", "bind", index, mac, channel]
    for prefix in (["sudo", "-n"], []):
        try:
            done = subprocess.run(prefix + cmd, capture_output=True, text=True, timeout=10)
        except (OSError, subprocess.SubprocessError):
            continue
        if done.returncode == 0 and os.path.exists(f"/dev/rfcomm{index}"):
            print(C.dim(f"SPP 已绑定 /dev/rfcomm{index} -> {mac} 通道 {channel}"))
            return f"/dev/rfcomm{index}"
    raise SystemExit(
        f"绑定 /dev/rfcomm 失败（{mac} 通道 {channel}，需要 root）。\n"
        "  不需要 root 的做法：直接用 --spp，它走 AF_BLUETOOTH socket，不建 TTY。"
    )


def _precommands(args, dev: Device) -> None:
    """按链路决定是否握手：BLE 不发 HELLO，服务模式由服务自己维持链路。"""
    if args.ble:
        print(C.dim("BLE 链路：固件约定不发 HELLO，直接 STATUS"))
        return
    if args.service:
        return
    dev.handshake()


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="AgentCore-Light 协议测试工具（USB 串口 / 蓝牙 SPP / BLE NUS / 厂商服务透传）",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="例子：\n"
        "  %(prog)s status\n"
        "  %(prog)s state BUSY\n"
        "  %(prog)s brightness 60\n"
        "  %(prog)s sound CLASSIC\n"
        "  %(prog)s monitor 10\n"
        "  %(prog)s --service demo      # 厂商服务在跑时用这种\n"
        "  %(prog)s --ble status        # 蓝牙 BLE（NUS），需拔掉 USB 让设备广播\n"
        "  %(prog)s --spp AA:BB:CC:DD:EE:FF state BUSY   # 蓝牙 SPP\n",
    )
    parser.add_argument("-p", "--port", help="串口设备，默认自动探测 /dev/ttyACM*")
    parser.add_argument("--service", action="store_true", help="通过厂商服务 127.0.0.1:18792 透传")
    parser.add_argument("--ble", action="store_true", help="走 BLE NUS（需要 bleak，且设备在广播）")
    parser.add_argument("--address", help="BLE 直连地址（AA:BB:...，跳过扫描）")
    parser.add_argument("--name-prefix", default="AgentCore-Light", help="BLE 扫描名字前缀")
    parser.add_argument("--scan-timeout", type=float, default=10.0, help="BLE 扫描时长（秒）")
    parser.add_argument(
        "--spp", metavar="MAC[:CH]", help="蓝牙 SPP 直连（AF_BLUETOOTH socket，免 root；默认自动扫通道）"
    )
    parser.add_argument(
        "--spp-bind", action="store_true", help="改走 root 的 rfcomm bind + /dev/rfcommN（可选）"
    )
    parser.add_argument("--url", default=SERVICE_URL, help="厂商服务地址")
    parser.add_argument("--log", default=DEFAULT_LOG, help="服务日志路径（服务模式跟踪设备输出用）")
    parser.add_argument("--force", action="store_true", help="厂商服务在跑时仍然直连串口（会互相抢读）")
    sub = parser.add_subparsers(dest="cmd", required=True)

    sub.add_parser("status", help="打印 STATUS + SETTINGS")
    p = sub.add_parser("state", help="下发灯效状态")
    p.add_argument("name")
    p = sub.add_parser("brightness", help="亮度 0-100%%")
    p.add_argument("percent", type=int)
    p = sub.add_parser("buzzer", help="蜂鸣器开关/音量")
    p.add_argument("enabled", choices=["on", "off"])
    p.add_argument("volume", nargs="?", type=int)
    p = sub.add_parser("lowbat", help="低电量提示开关")
    p.add_argument("enabled", choices=["on", "off"])
    p = sub.add_parser("sound", help="播放音效")
    p.add_argument("name")
    p.add_argument("mode", nargs="?", default="once", choices=["once", "loop"])
    p = sub.add_parser("sleep", help="休眠参数（秒）")
    p.add_argument("idle", nargs="?", type=int)
    p.add_argument("disconnect", nargs="?", type=int)
    p = sub.add_parser("name", help="改蓝牙名（AgentCore-Light-xxx）")
    p.add_argument("name")
    p = sub.add_parser("raw", help="发原始命令行")
    p.add_argument("line")
    p = sub.add_parser("transport", help="切换厂商服务的链路（serial / bluetooth）")
    p.add_argument("mode", choices=["serial", "bluetooth"])
    p = sub.add_parser("monitor", help="被动监听设备输出")
    p.add_argument("seconds", nargs="?", type=float, default=10.0)
    p = sub.add_parser("demo", help="完整演示并还原设置")
    p.add_argument("--quick", action="store_true")

    args = parser.parse_args(argv)

    link = None
    if args.ble:
        try:
            link = BleLink(args.address, args.name_prefix, args.scan_timeout)
        except SystemExit:
            raise
        except Exception as exc:
            raise SystemExit(C.red(f"BLE 连接失败：{exc}"))
        dev = Device(link, skip_hello=True)
        print(C.dim(f"BLE NUS 已连接 {link.address}"))
    elif args.spp:
        mac, channel = parse_spp_spec(args.spp)
        if args.spp_bind:
            port = spp_bind(args.spp)
            link = SerialLink(port)
            print(C.dim(f"蓝牙 SPP {port} @115200 8N1（经 rfcomm TTY）"))
        else:
            link = RfcommLink(mac, channel)
            print(
                C.dim(
                    f"蓝牙 SPP socket 已连接 {mac} 通道 {link.channel}"
                    "（免 root，不创建 /dev/rfcomm*）"
                )
            )
        dev = Device(link, skip_hello=False)
    elif args.service:
        link = ServiceLink(args.url, args.log)
        dev = Device(link, skip_hello=True)
    else:
        port = find_port(args.port)
        url = args.url.rstrip("/")
        try:
            with urllib.request.urlopen(f"{url}/health", timeout=0.3) as resp:
                resp.read()
            print(
                C.yellow(
                    f"警告：厂商后台服务（{url}）正在运行，它可能同时持有 {port}。"
                    "\n      Linux tty 默认允许多次 open，两个进程会抢读同一份字节，"
                    "\n      导致服务侧漏数据/误判断链。推荐改用 --service。"
                )
            )
            if not args.force:
                raise SystemExit(C.dim("已中止。如确认要直连（例如已停服务但端口仍被占），加 --force。"))
        except SystemExit:
            raise
        except Exception:
            pass
        try:
            link = SerialLink(port)
        except OSError as exc:
            if exc.errno == errno.EBUSY:
                raise SystemExit(
                    f"{port} 被占用。\n"
                    f"  1) 改用 --service 走厂商服务透传，或\n"
                    f"  2) 先停服务：AgentCore-Light stop"
                )
            if exc.errno == errno.EACCES:
                raise SystemExit(f"{port} 权限不足：sudo usermod -aG dialout $USER 后重新登录")
            raise
        dev = Device(link, skip_hello=False)
        print(C.dim(f"直连 {port} @115200 8N1"))

    try:
        if args.cmd == "status":
            _precommands(args, dev)
            dev.status()
            dev.settings()
        elif args.cmd == "state":
            _precommands(args, dev)
            cmd_state(dev, args.name)
        elif args.cmd == "brightness":
            _precommands(args, dev)
            cmd_brightness(dev, args.percent)
        elif args.cmd == "buzzer":
            _precommands(args, dev)
            cmd_buzzer(dev, args.enabled, args.volume)
        elif args.cmd == "lowbat":
            _precommands(args, dev)
            cmd_lowbat(dev, args.enabled)
        elif args.cmd == "sound":
            _precommands(args, dev)
            cmd_sound(dev, args.name, args.mode)
        elif args.cmd == "sleep":
            _precommands(args, dev)
            cmd_sleep(dev, args.idle, args.disconnect)
        elif args.cmd == "name":
            _precommands(args, dev)
            cmd_name(dev, args.name)
        elif args.cmd == "raw":
            _precommands(args, dev)
            cmd_raw(dev, args.line)
        elif args.cmd == "transport":
            cmd_transport(args, args.mode)
        elif args.cmd == "monitor":
            _precommands(args, dev)
            monitor(dev, args.seconds)
        elif args.cmd == "demo":
            demo(dev, quick=args.quick)
    except KeyboardInterrupt:
        print("\n中断")
        return 130
    finally:
        link.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
