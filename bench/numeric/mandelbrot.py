# mandelbrot の Python の実装。bench/mandelbrot.em と同じ順で浮動小数の演算を行い、集合に入る格子点の数を書く
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。`complex` は掛け算の演算の順が決まらないので使わない
import sys


def inside(cr, ci):
    zr = 0.0
    zi = 0.0
    for _ in range(50):
        zr, zi = (zr * zr - zi * zi) + cr, (zr * zi + zi * zr) + ci
        if zr * zr + zi * zi > 4.0:
            return False
    return True


def main():
    n = int(sys.argv[1])
    if n < 1:
        sys.exit("usage: mandelbrot.py SIZE")
    size = float(n)
    count = 0
    for y in range(n):
        ci = 2.0 * float(y) / size - 1.0
        for x in range(n):
            cr = 2.0 * float(x) / size - 1.5
            if inside(cr, ci):
                count += 1
    print(count)


main()
