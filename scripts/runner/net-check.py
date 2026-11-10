#!/usr/bin/env python3
"""Boots one microVM the way the daemon does and checks its network from inside.

Run on the runner host as the runner user, after host-setup.sh and build-base-image.sh, with a
tools disk the daemon made (the newest /srv/runner/vms/tools-*.ext4):

    python3 scripts/runner/net-check.py [slot, default 900]

Expected: the internet works (DNS and HTTPS), and the host's other ports, private ranges, the
cloud metadata address and other VMs are unreachable.
"""
import glob, json, os, socket, struct, subprocess, sys, tempfile, time

SLOT = int(sys.argv[1]) if len(sys.argv) > 1 else 900
base = SLOT * 4
HOST = f"10.200.{base >> 8}.{(base & 255) + 1}"
GUEST = f"10.200.{base >> 8}.{(base & 255) + 2}"
TAP = f"rvm{SLOT}"
PUBLIC = subprocess.run(["sh", "-c", "ip -4 route get 1.1.1.1 | sed -n 's/.* src \\([0-9.]*\\).*/\\1/p'"],
                        capture_output=True, text=True).stdout.strip()

CHECKS = f"""
ok() {{ echo "PASS $1"; }}; bad() {{ echo "FAIL $1"; }}
try() {{ timeout 5 "$@" >/dev/null 2>&1; }}
try getent hosts example.com && ok "dns resolves" || bad "dns resolves"
try curl -fsS https://example.com && ok "https to the internet" || bad "https to the internet"
try curl -fsS http://{HOST}:22 && bad "host ssh reachable" || ok "host ssh blocked"
try curl -fsS http://{PUBLIC}:22 && bad "host public ip reachable" || ok "host public ip blocked"
try curl -fsS http://169.254.169.254/ && bad "metadata reachable" || ok "metadata blocked"
try curl -fsS http://192.168.1.1/ && bad "lan reachable" || ok "lan blocked"
try curl -fsS http://10.200.0.2/ && bad "other vm reachable" || ok "other vm blocked"
"""


def frame(tag, payload=b""):
    return struct.pack(">BI", tag, len(payload)) + payload


def main():
    tools = max(glob.glob("/srv/runner/vms/tools-*.ext4") + glob.glob("/srv/runner/*/vms-*/tools-*.ext4"), key=os.path.getmtime)
    d = tempfile.mkdtemp(prefix="netcheck-", dir="/srv/runner/vms")
    helper = ["sudo", "-n", "/usr/local/sbin/runner-vm"]
    subprocess.run(helper + ["down", str(SLOT)])
    subprocess.run(helper + ["up", str(SLOT)], check=True)
    scratch = f"{d}/scratch.ext4"
    with open(scratch, "wb") as f:
        f.truncate(4 << 30)
    subprocess.run(["mkfs.ext4", "-q", "-F", scratch], check=True)
    cfg = {
        "boot-source": {"kernel_image_path": "/srv/runner/kernels/vmlinux",
                        "boot_args": f"console=ttyS0 reboot=k panic=1 pci=off ro init=/sbin/vm-init ip={GUEST}::{HOST}:255.255.255.252::eth0:off"},
        "drives": [
            {"drive_id": "root", "path_on_host": os.path.realpath("/srv/runner/images/base.ext4"), "is_root_device": True, "is_read_only": True},
            {"drive_id": "tools", "path_on_host": tools, "is_root_device": False, "is_read_only": True},
            {"drive_id": "scratch", "path_on_host": scratch, "is_root_device": False, "is_read_only": False},
        ],
        "network-interfaces": [{"iface_id": "eth0", "host_dev_name": TAP, "guest_mac": "06:00:0a:c8:ff:02"}],
        "vsock": {"guest_cid": 3, "uds_path": f"{d}/v.sock"},
        "machine-config": {"vcpu_count": 1, "mem_size_mib": 512},
    }
    json.dump(cfg, open(f"{d}/vm.json", "w"))
    fc = subprocess.Popen(["firecracker", "--api-sock", f"{d}/api.sock", "--config-file", f"{d}/vm.json"],
                          stdout=open(f"{d}/console.log", "w"), stderr=subprocess.STDOUT)
    try:
        deadline = time.time() + 30
        while True:
            try:
                s = socket.socket(socket.AF_UNIX)
                s.connect(f"{d}/v.sock")
                s.sendall(b"CONNECT 1024\n")
                if s.recv(64).startswith(b"OK"):
                    break
            except OSError:
                pass
            if time.time() > deadline:
                sys.exit("the VM did not start")
            time.sleep(0.1)
        start = {"program": "sh", "args": ["-c", CHECKS], "env": [["PATH", "/usr/local/bin:/usr/bin:/bin"], ["HOME", "/tmp"]],
                 "cwd": "/", "stderr": "/tmp/stderr.log", "uid": 1000, "gid": 1000,
                 "hostIp": HOST, "guestIp": GUEST, "mounts": [], "chatRunSocket": None}
        s.sendall(frame(1, json.dumps(start).encode()))
        out = b""
        while True:
            head = s.recv(5, socket.MSG_WAITALL)
            if len(head) < 5:
                break
            tag, n = struct.unpack(">BI", head)
            body = s.recv(n, socket.MSG_WAITALL) if n else b""
            if tag == 101:
                out += body
            elif tag == 104:
                print("agent error:", body.decode())
            elif tag == 103:
                break
        text = out.decode()
        print(text, end="")
        sys.exit(1 if "FAIL" in text or "PASS" not in text else 0)
    finally:
        fc.kill()
        fc.wait()
        subprocess.run(helper + ["down", str(SLOT)])
        subprocess.run(["rm", "-rf", d])


main()
