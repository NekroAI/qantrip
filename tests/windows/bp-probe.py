# Bracketed-paste probe for Windows Terminal: enable VT input + bracketed paste, record raw input bytes.
import ctypes, msvcrt, sys, time
k = ctypes.windll.kernel32
h = k.GetStdHandle(-10)
k.SetConsoleMode(h, 0x200)  # ENABLE_VIRTUAL_TERMINAL_INPUT only (no line input / echo / processed input)
sys.stdout.write("\x1b[?2004hbp-probe ready (bracketed paste on)\r\n")
sys.stdout.flush()
buf = []
end = time.time() + float(sys.argv[2])
while time.time() < end:
    if msvcrt.kbhit():
        buf.append(msvcrt.getwch())
    else:
        time.sleep(0.005)
sys.stdout.write("\x1b[?2004l")
with open(sys.argv[1], "w", encoding="utf-8") as f:
    f.write(repr("".join(buf)))
