"""Minimal usign(1) signer/verifier compatible with OpenWrt's usign tool.

Format (signify-derivative, no KDF):

  Public key (42 bytes raw):
    [0:2]   pkalg  = "Ed"
    [2:10]  keynum (8 bytes, random fingerprint)
    [10:42] ed25519 public key (32 bytes)

  Secret key (104 bytes raw, no passphrase):
    [0:2]   pkalg   = "Ed"
    [2:4]   kdfalg  = "BK"
    [4:8]   kdfrounds = 0
    [8:24]  salt (16 bytes, unused at rounds=0)
    [24:32] checksum = SHA512(privkey)[:8]
    [32:40] keynum
    [40:104] ed25519 secret key (64 bytes, libsodium concat seed+pub)

  Signature (74 bytes raw):
    [0:2]   pkalg  = "Ed"
    [2:10]  keynum
    [10:74] ed25519 signature (64 bytes)

Each on-disk file is two lines:
  untrusted comment: <free text>
  <base64(payload)>
"""
from __future__ import annotations

import base64
import hashlib
from pathlib import Path

from cryptography.hazmat.primitives.asymmetric import ed25519


# Header magic for files we emit.
_PKALG_ED = b"Ed"


def _read_usign_file(path: str | Path) -> tuple[str, bytes]:
    """Return (comment, raw_payload) for a usign key/signature file."""
    text = Path(path).read_text(encoding="utf-8")
    lines = [ln for ln in text.splitlines() if ln.strip()]
    if len(lines) < 2:
        raise ValueError(f"{path}: expected `comment\\n<base64>` (got {len(lines)} lines)")
    comment_line = lines[0]
    if comment_line.startswith("untrusted comment:"):
        comment = comment_line[len("untrusted comment:"):].strip()
    else:
        comment = comment_line.strip()
    payload = base64.b64decode(lines[1])
    return comment, payload


def _write_usign_file(path: str | Path, comment: str, payload: bytes, mode: int = 0o644) -> None:
    """Write a usign-style two-line file (comment + base64)."""
    content = f"untrusted comment: {comment}\n{base64.b64encode(payload).decode('ascii')}\n"
    p = Path(path)
    p.write_text(content, encoding="utf-8", newline="\n")
    p.chmod(mode)


def load_public_key(path: str | Path) -> tuple[bytes, ed25519.Ed25519PublicKey]:
    """Parse a usign public key file. Returns (keynum, public_key)."""
    _, raw = _read_usign_file(path)
    if len(raw) != 42 or raw[0:2] != _PKALG_ED:
        raise ValueError(f"{path}: not a usign Ed25519 public key (len={len(raw)})")
    keynum = raw[2:10]
    pubkey = ed25519.Ed25519PublicKey.from_public_bytes(raw[10:42])
    return keynum, pubkey


def load_secret_key(path: str | Path) -> tuple[bytes, ed25519.Ed25519PrivateKey]:
    """Parse a usign secret key file. Returns (keynum, private_key)."""
    _, raw = _read_usign_file(path)
    if len(raw) != 104 or raw[0:2] != _PKALG_ED:
        raise ValueError(f"{path}: not a usign Ed25519 secret key (len={len(raw)})")
    if raw[2:4] != b"BK":
        raise ValueError(f"{path}: unexpected kdfalg {raw[2:4]!r}")
    if int.from_bytes(raw[4:8], "big") != 0:
        raise ValueError(f"{path}: KDF-protected secret keys not supported (rounds!=0)")
    cksum_stored = raw[24:32]
    keynum = raw[32:40]
    sk_bytes = raw[40:104]  # 64-byte libsodium-style: seed(32) + pubkey(32)
    if hashlib.sha512(sk_bytes).digest()[:8] != cksum_stored:
        raise ValueError(f"{path}: secret-key checksum mismatch")
    seed = sk_bytes[:32]
    return keynum, ed25519.Ed25519PrivateKey.from_private_bytes(seed)


def sign_bytes(message: bytes, sec_path: str | Path) -> bytes:
    """Sign `message` with a usign secret key. Returns the raw 74-byte signature payload."""
    keynum, sk = load_secret_key(sec_path)
    sig = sk.sign(message)  # raw 64-byte ed25519 signature
    return _PKALG_ED + keynum + sig


def sign_file(message_path: str | Path, sec_path: str | Path,
              sig_path: str | Path | None = None) -> Path:
    """Sign a file, writing `<message>.sig` (or sig_path if provided)."""
    msg = Path(message_path).read_bytes()
    payload = sign_bytes(msg, sec_path)
    keynum_hex = payload[2:10].hex()
    out = Path(sig_path) if sig_path else Path(str(message_path) + ".sig")
    _write_usign_file(out, f"signed by key {keynum_hex}", payload, mode=0o644)
    return out


def verify_bytes(message: bytes, sig_payload: bytes, pub_path: str | Path) -> bool:
    """Return True if `sig_payload` (raw 74 bytes) verifies `message` under pub_path."""
    keynum_pub, pubkey = load_public_key(pub_path)
    if len(sig_payload) != 74 or sig_payload[0:2] != _PKALG_ED:
        return False
    if sig_payload[2:10] != keynum_pub:
        return False
    try:
        pubkey.verify(sig_payload[10:74], message)
        return True
    except Exception:
        return False


def verify_file(message_path: str | Path, sig_path: str | Path,
                pub_path: str | Path) -> bool:
    """Verify a file against `<sig>` using public key `<pub>`."""
    msg = Path(message_path).read_bytes()
    _, sig_payload = _read_usign_file(sig_path)
    return verify_bytes(msg, sig_payload, pub_path)


if __name__ == "__main__":
    # Quick round-trip smoke test against the project keys.
    import sys
    KEYS = Path(__file__).parent / "keys"
    PUB = KEYS / "release.usign.pub"
    SEC = KEYS / "release.usign.sec"
    if not (PUB.exists() and SEC.exists()):
        sys.exit("[selftest] missing keys/release.usign.{pub,sec}")
    msg = b"hello usign\n"
    payload = sign_bytes(msg, SEC)
    assert verify_bytes(msg, payload, PUB), "self-verify failed"
    assert not verify_bytes(b"tampered", payload, PUB), "tamper-detection failed"
    print(f"[selftest] sign + verify OK (sig {len(payload)} B, keynum {payload[2:10].hex()})")
