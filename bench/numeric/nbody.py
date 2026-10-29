# nbody の Python の実装。bench/nbody.em と同じ順で浮動小数の演算を行い、進める前と後のエネルギーを書く
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import math
import sys

SOLAR_MASS = 39.47841760435743
DAYS_PER_YEAR = 365.24
DT = 0.01


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def add(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def scale(v, k):
    return [v[0] * k, v[1] * k, v[2] * k]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def bodies():
    pos = [
        [0.0, 0.0, 0.0],
        [4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01],
        [8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01],
        [1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01],
        [1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01],
    ]
    vel = [
        [0.0, 0.0, 0.0],
        [1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05],
        [-2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05],
        [2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05],
        [2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05],
    ]
    vel = [scale(v, DAYS_PER_YEAR) for v in vel]
    mass = [
        SOLAR_MASS,
        9.54791938424326609e-04 * SOLAR_MASS,
        2.85885980666130812e-04 * SOLAR_MASS,
        4.36624404335156298e-05 * SOLAR_MASS,
        5.15138902046611451e-05 * SOLAR_MASS,
    ]
    return pos, vel, mass


def offset_momentum(vel, mass):
    p = [0.0, 0.0, 0.0]
    for i in range(len(mass)):
        p = add(p, scale(vel[i], mass[i]))
    vel[0] = [0.0 - p[0] / SOLAR_MASS, 0.0 - p[1] / SOLAR_MASS, 0.0 - p[2] / SOLAR_MASS]


def energy(pos, vel, mass):
    e = 0.0
    n = len(mass)
    for i in range(n):
        e = e + 0.5 * mass[i] * dot(vel[i], vel[i])
        for j in range(i + 1, n):
            d = sub(pos[i], pos[j])
            e = e - mass[i] * mass[j] / math.sqrt(dot(d, d))
    return e


def advance(pos, vel, mass):
    n = len(mass)
    for i in range(n):
        for j in range(i + 1, n):
            d = sub(pos[i], pos[j])
            d2 = dot(d, d)
            mag = DT / (d2 * math.sqrt(d2))
            vel[i] = sub(vel[i], scale(d, mass[j] * mag))
            vel[j] = add(vel[j], scale(d, mass[i] * mag))
    for i in range(n):
        pos[i] = add(pos[i], scale(vel[i], DT))


def main():
    steps = int(sys.argv[1])
    if steps < 1:
        sys.exit("usage: nbody.py SIZE")
    pos, vel, mass = bodies()
    offset_momentum(vel, mass)
    print(f"{energy(pos, vel, mass):.9f}")
    for _ in range(steps):
        advance(pos, vel, mass)
    print(f"{energy(pos, vel, mass):.9f}")


main()
