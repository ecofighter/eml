"""計測の相手 (ブロックする socket の Python)。サーバーは待ち受けたポートを標準出力の1行目に出す。

python3 -I peer.py echo-server
python3 -I peer.py sink-server
python3 -I peer.py source-server MIB CHUNK_KIB
python3 -I peer.py many-server C
python3 -I peer.py conn-server N
python3 -I peer.py echo-client N SIZE PORT     (Python 同士の床を測るためのクライアント)
python3 -I peer.py bulk-send MIB CHUNK_KIB PORT
python3 -I peer.py conn-client N PORT
"""
import socket
import sys


def listener():
    l = socket.socket()
    l.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    l.bind(("127.0.0.1", 0))
    l.listen(4096)
    print(l.getsockname()[1], flush=True)
    return l


def nodelay(s):
    s.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    return s


def main():
    mode, args = sys.argv[1], [int(a) for a in sys.argv[2:]]
    if mode == "echo-server":
        s, _ = listener().accept()
        nodelay(s)
        while True:
            b = s.recv(65536)
            if not b:
                break
            s.sendall(b)
        s.close()
    elif mode == "sink-server":
        s, _ = listener().accept()
        while s.recv(1 << 20):
            pass
        s.close()
    elif mode == "source-server":
        mib, kib = args
        s, _ = listener().accept()
        buf = b"x" * (kib * 1024)
        for _ in range(mib * 1024 // kib):
            s.sendall(buf)
        s.close()
    elif mode == "many-server":
        (c,) = args
        l = listener()
        xs = [l.accept()[0] for _ in range(c)]
        for s in xs:
            s.sendall(s.recv(65536))
        for s in xs:
            s.recv(65536)
            s.close()
    elif mode == "conn-server":
        (n,) = args
        l = listener()
        for _ in range(n):
            l.accept()[0].close()
    elif mode == "echo-client":
        n, size, port = args
        s = nodelay(socket.create_connection(("127.0.0.1", port)))
        msg = b"x" * size
        for _ in range(n):
            s.sendall(msg)
            got = 0
            while got < size:
                got += len(s.recv(65536))
        s.close()
    elif mode == "bulk-send":
        mib, kib, port = args
        s = socket.create_connection(("127.0.0.1", port))
        buf = b"x" * (kib * 1024)
        for _ in range(mib * 1024 // kib):
            s.sendall(buf)
        s.close()
    elif mode == "conn-client":
        n, port = args
        for _ in range(n):
            s = socket.create_connection(("127.0.0.1", port))
            s.recv(1)
            s.close()
    else:
        sys.exit(f"unknown mode {mode}")


main()
