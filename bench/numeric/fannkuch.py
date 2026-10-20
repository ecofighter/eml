# fannkuch-redux の Python の版。bench/fannkuch_var.em のループを1つずつ写し、同じ2行を書く
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import sys


def fannkuch(n):
    perm1 = list(range(n))
    perm = [0] * n
    count = [0] * n
    max_flips = 0
    checksum = 0
    parity = 0
    r = n
    done = False
    while not done:
        while r != 1:
            count[r - 1] = r
            r -= 1
        for i in range(n):
            perm[i] = perm1[i]
        flips = 0
        k = perm[0]
        while k != 0:
            lo = 0
            hi = k
            while lo < hi:
                perm[lo], perm[hi] = perm[hi], perm[lo]
                lo += 1
                hi -= 1
            flips += 1
            k = perm[0]
        if flips > max_flips:
            max_flips = flips
        checksum += flips if parity % 2 == 0 else 0 - flips
        more = True
        while more:
            if r == n:
                done = True
                more = False
            else:
                perm0 = perm1[0]
                for i in range(r):
                    perm1[i] = perm1[i + 1]
                perm1[r] = perm0
                count[r] -= 1
                if count[r] > 0:
                    more = False
                else:
                    r += 1
        parity += 1
    print(checksum)
    print(f"Pfannkuchen({n}) = {max_flips}")


fannkuch(int(sys.argv[1]))
