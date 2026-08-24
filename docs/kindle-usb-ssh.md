# Kindle over USB: SSH + flashing on a new machine

Goal: plug the Kindle into a Linux box with a USB cable and be able to run
`./flash.sh` against it, with the setup surviving reboots, replugs, and
interface renames.

Two independent halves:

- **Kindle side** — the USBNetwork jailbreak hack must be installed and enabled.
  This lives on the device, so it is done once and then travels with the Kindle to
  any machine. See [§1](#1-kindle-side).
- **Host side** — the machine needs an IP on the USB network segment. This is
  per-machine and is the part you redo on `arch-pad`. See [§2](#2-host-side-arch-linux).

> **Provenance.** Everything in §2 and §3 was read off the current dev machine's live
> configuration and is known-good. §1 is written from the MobileRead `usbnetwork`
> package's conventions and is marked as *verify on device* — the Kindle was not
> plugged in when this was written, so the exact paths should be confirmed rather
> than trusted.

## The addressing scheme

| Endpoint | Address |
|---|---|
| Kindle (usbnet gadget) | `192.168.15.244` |
| Host (this side of the cable) | `192.168.15.1/24` |
| SSH | port 22, user `root` |

`192.168.15.244` is what `flash.sh` scp's to and what has a host key in
`~/.ssh/known_hosts`, so it is the address actually in use. The usbnet package's own
default for the host end is `192.168.15.201`; `.1` works just as well because it is the
same `/24` and nothing routes.

---

## 1. Kindle side

*This is already done on the device — it is here so it can be re-done or debugged.
**Verify these paths on the device rather than assuming them.***

The jailbreak component is MobileRead's **USBNetwork** hack (`kindle-usbnetwork`), which
installs to `/mnt/us/usbnet/` and runs a dropbear SSH daemon over the USB ethernet gadget.

Things to check when SSH does not come up:

```sh
# From a KUAL menu on the device, or over SSH if you can already get in:
ls /mnt/us/usbnet/                  # the hack must be installed at all
ls /mnt/us/usbnet/auto              # presence of this file = enable at boot
cat /mnt/us/usbnet/etc/config       # host/device MAC + address settings
ls /mnt/us/usbnet/etc/authorized_keys
```

### Enabling it

- **KUAL** — the USBNetwork extension adds a menu with toggle entries
  ("Toggle USBNetworking" / status). This is the normal way.
- **Search-bar debug command** — typing `;un` into the Kindle's search field toggles
  usbnet on older firmware.

### Persistence — the part that keeps biting

Toggling via the KUAL menu is **for the current session only**. On reboot it reverts.
The persistence switch is the marker file:

```sh
touch /mnt/us/usbnet/auto     # enable usbnet automatically at every boot
rm    /mnt/us/usbnet/auto     # back to manual toggling
```

So: if SSH works right after you toggle it but is gone after a restart, that file is
missing. That is almost certainly the "port-opening isn't persistent" symptom.

### Key-based login

Append your public key to `/mnt/us/usbnet/etc/authorized_keys` on the device (note: **not**
`/root/.ssh/authorized_keys` — the usbnet dropbear instance uses its own path). This
matters more than convenience: `flash.sh` scp's a whole directory tree and will prompt for
a password per file otherwise.

---

## 2. Host side (Arch Linux)

The problem to solve: the USB ethernet gadget shows up with an unstable interface name
(`usb0`, `enp0s20f0u3`, `enp…u1c2`, …) that changes with the physical port and between
machines. Anything keyed on the interface *name* will break.

### The fix: match on MAC, not on name

The Kindle's usbnet gadget presents a fixed MAC on the host side. systemd-networkd can
match on that, so the interface name becomes irrelevant.

Create `/etc/systemd/network/10-kindle.network` (root, `0644`):

```ini
[Match]
MACAddress=ee:49:00:00:00:00

[Network]
Address=192.168.15.1/24
```

Then:

```sh
sudo systemctl enable --now systemd-networkd
sudo systemctl restart systemd-networkd
```

That is the whole host-side setup, and it is **portable**: the same three lines work on any
machine, on any USB port, under any interface name, because the MAC belongs to the Kindle.
This exact file is what is running on the current dev machine.

> `ee:49:00:00:00:00` is the host-side MAC this particular Kindle's gadget presents (set by
> `g_ether`'s `host_addr` parameter, configured in `/mnt/us/usbnet/etc/config`). Confirm it
> after plugging in with `ip -br link`; if it differs, either update the file or change the
> Kindle-side config so all your machines agree.
>
> If the MAC ever turns out to be unstable, the fallback match is by driver instead:
> ```ini
> [Match]
> Driver=cdc_ether cdc_ncm rndis_host
> ```
> — less precise (it would also match other USB-ethernet devices), but name-independent.

### If NetworkManager is also running

Both NetworkManager and systemd-networkd are active on the current dev machine — NM owns
the real ethernet, networkd owns the Kindle link. That works, but NM can race to claim the
USB interface first. If it does, tell NM to leave it alone:

`/etc/NetworkManager/conf.d/99-kindle-unmanaged.conf`

```ini
[keyfile]
unmanaged-devices=mac:ee:49:00:00:00:00
```

then `sudo systemctl reload NetworkManager`.

### Name resolution

`/etc/hosts`:

```
192.168.15.244	kindle
```

### `~/.ssh/config`

```
Host kindle
    Hostname 192.168.15.244
    Port     22
    User     root
```

> ⚠ **Fix this when you copy it.** The current dev machine has
> `Hostname 169.254.7.190` in this block — a stale link-local address that is not on the
> usbnet segment. Because `ssh_config` takes precedence over `/etc/hosts`, `ssh kindle`
> resolves to the dead address while `scp root@192.168.15.244` (what `flash.sh` uses)
> works. Set it to `192.168.15.244` on `arch-pad` so `ssh kindle` and `flash.sh` agree
> about how to reach the device.

---

## 3. Verifying the link

Plug the cable in, then, in order:

```sh
ip -br link                       # find the new interface; note its MAC
ip -br addr                       # it should carry 192.168.15.1/24
networkctl status                 # networkd should show it as configured
ping -c3 192.168.15.244           # the Kindle
ssh kindle                        # or: ssh root@192.168.15.244
```

Failure decision tree:

| Symptom | Cause | Fix |
|---|---|---|
| No new interface at all | usbnet not enabled on the device, or the cable is charge-only | Toggle usbnet via KUAL; try another cable |
| Interface appears, no IP | MAC in `10-kindle.network` doesn't match | `ip -br link`, update the `[Match]` |
| Interface has an IP but ping fails | Kindle isn't on `.244`, or NM hijacked the link | Check `/mnt/us/usbnet/etc/config`; add the NM unmanaged rule |
| Ping works, SSH refused | dropbear isn't running | Re-toggle usbnet on the device |
| SSH asks for a password | key not in `/mnt/us/usbnet/etc/authorized_keys` | Append it |
| Worked before, gone after reboot | `/mnt/us/usbnet/auto` missing | `touch` it |

---

## 4. Flashing

Once SSH is up:

```sh
./flash.sh
```

which does: `cross build --release` (armv7-musl) → copy the binary into
`kindle_KUAL/hellokindle/bin/` → wipe local logs → **move `secrets/token.json` aside** →
`scp -r ./kindle_KUAL/* root@192.168.15.244:/mnt/us` → move the token back.

That token dance is deliberate: it stops the deploy from overwriting the OAuth token
already stored on the device. **Do not break it when editing these scripts.**

Prerequisites on the host:

```sh
rustup target add armv7-unknown-linux-musleabi
cargo install cross --git https://github.com/cross-rs/cross   # needs Docker or Podman
```

### Alternatives

- **`flash_usb.sh`** — no SSH at all. Mounts the Kindle as USB mass storage and copies the
  tree. Note it hard-codes `/dev/sdb1` → `/mnt/tmp`; the block device will differ on
  `arch-pad`, so check `lsblk` first. (The path in `CLAUDE.md`,
  `/run/media/mxy/Kindle`, is out of date relative to the script.)
- **`flash_display.sh`** — deleted in `916dd0f`. It deployed a `test_ui` binary that no
  longer exists as a `[[bin]]` target.

The IP in `flash.sh` is hard-coded (`root@192.168.15.244`). If you fix `~/.ssh/config` as
described above, changing it to `root@kindle` makes it machine-independent.

### After flashing

On the device: KUAL → **Kindli-Chess → ChessApp**, which runs
`/mnt/us/hellokindle/chess_app.sh`. That script also performs the **update trampoline** —
if the in-app updater staged a verified binary at `bin/kindle-hello.new`, it is moved into
place here, while no instance is running. See [`ci-cd.md`](ci-cd.md).

Logs to read when something goes wrong on-device:

```
/mnt/us/hellokindle/log/app.log      # written by the binary (log4rs)
/mnt/us/hellokindle/log/launch.log   # written by chess_app.sh
```

---

## 5. Note on the QR authentication flow

The Lichess OAuth flow starts an axum server **on the Kindle** at
`http://<kindle-lan-ip>:8080/callback` and shows a QR code for a phone to scan. The
redirect target is the address returned by `local_ip_address::local_ip()`.

So authentication needs the Kindle on **Wi-Fi**, reachable from the phone — the USB link is
point-to-point between the Kindle and one host, and the phone has no route to it. USB
networking is for flashing and debugging; Wi-Fi is for actually using the app.

Once a token is stored at `/mnt/us/hellokindle/secrets/token.json` it is reused silently,
so this only matters on first auth or after the token is rejected.
