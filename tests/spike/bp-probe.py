#!/usr/bin/env python3
"""测试辅助：在终端中开启 bracketed paste，原样记录收到的字节，判断粘贴是否被包裹。"""
import os, sys, termios, tty, select, time
out = os.path.expanduser("~/Library/Logs/Qantrip/bp-probe.bin")
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
sys.stdout.write("\x1b[?2004h bracketed paste probe: waiting 15s...\r\n"); sys.stdout.flush()
open(out, "wb").close()
try:
    tty.setraw(fd)
    end = time.time() + 15
    with open(out, "ab") as f:
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], 0.2)
            if r:
                f.write(os.read(fd, 65536)); f.flush()
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
    sys.stdout.write("\x1b[?2004l\r\ndone\r\n")
