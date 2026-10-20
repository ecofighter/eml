# エラトステネスのふるいの Python の版。bench/sieve_var.em と同じく、引数 N より小さい素数の数を書く
# (docs/implementation/benchmarks.md の「スクリプトの計測」)。
import sys


def sieve(n):
    marks = [True] * n
    for i in range(2, n):
        if marks[i]:
            for j in range(i * i, n, i):
                marks[j] = False
    return marks


def main():
    n = int(sys.argv[1])
    if n < 1:
        sys.exit("usage: sieve.py SIZE")
    marks = sieve(n)
    print(sum(1 for i in range(2, len(marks)) if marks[i]))


main()
