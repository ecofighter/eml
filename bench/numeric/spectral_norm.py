# spectral-norm の Python の版。bench/spectral_norm.em と同じ順で浮動小数の演算を行い、ノルムを書く
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import math
import sys


def a(i, j):
    return 1.0 / float((i + j) * (i + j + 1) // 2 + i + 1)


def mul_av(v):
    n = len(v)
    result = []
    for i in range(n):
        s = 0.0
        for j in range(n):
            s = s + a(i, j) * v[j]
        result.append(s)
    return result


def mul_atv(v):
    n = len(v)
    result = []
    for i in range(n):
        s = 0.0
        for j in range(n):
            s = s + a(j, i) * v[j]
        result.append(s)
    return result


def mul_atav(v):
    return mul_atv(mul_av(v))


def main():
    n = int(sys.argv[1])
    u = [1.0] * n
    v = u
    for _ in range(10):
        v = mul_atav(u)
        u = mul_atav(v)
    vbv = 0.0
    vv = 0.0
    for i in range(n):
        vbv = vbv + u[i] * v[i]
        vv = vv + v[i] * v[i]
    print(f"{math.sqrt(vbv / vv):.9f}")


main()
