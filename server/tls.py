"""HTTPS for phones and other devices on the local network.

Browsers only allow installing the app, the microphone, notifications and offline use on a secure origin, and plain
http://192.168.x.y is not one. LazyBoy makes its own certificate authority once, per data directory, and signs a
server certificate for this host's names and addresses with it. A device trusts the authority once
(/lazyboy-ca.crt); renewing the server certificate or a new address never asks again. The authority is limited by
name constraints to private addresses and local names, so even a leaked key cannot impersonate a public website.
"""
import contextlib
import datetime
import fcntl
import ipaddress
import logging
import os
import socket
import struct
from pathlib import Path

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

log = logging.getLogger("server.tls")

PRIVATE_NETWORKS = ["127.0.0.0/8", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "169.254.0.0/16", "100.64.0.0/10",
                    "::1/128", "fc00::/7", "fe80::/10"]
# Apple refuses server certificates valid for longer than 825 days, even from a private authority.
SERVER_DAYS = 397
RENEW_BEFORE = datetime.timedelta(days=30)
# Docker and VM bridges are internal to this host; no other device reaches LazyBoy there.
INTERNAL_INTERFACES = ("lo", "docker", "br-", "veth", "virbr", "cni", "flannel")


def local_addresses():
    """This host's IPv4 addresses that other devices can reach, from its network interfaces (Linux)."""
    found = []
    with contextlib.suppress(OSError), socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        for _, name in socket.if_nameindex():
            if name.startswith(INTERNAL_INTERFACES): continue
            with contextlib.suppress(OSError):
                packed = fcntl.ioctl(probe.fileno(), 0x8915, struct.pack("256s", name[:15].encode()))  # SIOCGIFADDR
                found.append(socket.inet_ntoa(packed[20:24]))
    return found


def host_name():
    """This computer's name. In Docker the container has its own, so `make up` passes the host's in LAZYHERMES_HOSTNAME."""
    return (os.environ.get("LAZYHERMES_HOSTNAME") or socket.gethostname()).split(".")[0].lower() or "localhost"


def public_hosts():
    """Names and addresses devices use to reach LazyBoy. In Docker the container cannot see the host's network,
    so `make up` passes the addresses in LAZYHERMES_PUBLIC_HOSTS."""
    given = [h.strip() for h in os.environ.get("LAZYHERMES_PUBLIC_HOSTS", "").split(",") if h.strip()]
    name = host_name()
    return [*(given or local_addresses()), *([name, name + ".local"] if name != "localhost" else [])]


def _is_ip(host):
    try: ipaddress.ip_address(host); return True
    except ValueError: return False


def _permitted(host, hostname):
    if _is_ip(host): return any(ipaddress.ip_address(host) in ipaddress.ip_network(n) for n in PRIVATE_NETWORKS)
    host = host.lower().rstrip(".")
    return host in {"localhost", hostname} or host.endswith(".local") or host.endswith("." + hostname)


def _now(): return datetime.datetime.now(datetime.UTC)


def _write(path, data, private=False):
    tmp = path.with_suffix(path.suffix + ".new")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600 if private else 0o644)
    with os.fdopen(fd, "wb") as out: out.write(data)
    os.replace(tmp, path)


def _key_bytes(key):
    return key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())


def _authority(directory, hostname):
    cert_path, key_path = directory / "ca.crt", directory / "ca.key"
    if cert_path.exists() and key_path.exists():
        return x509.load_pem_x509_certificate(cert_path.read_bytes()), serialization.load_pem_private_key(key_path.read_bytes(), None)
    key = ec.generate_private_key(ec.SECP256R1())
    name = x509.Name([x509.NameAttribute(NameOID.ORGANIZATION_NAME, "LazyBoy"),
                      x509.NameAttribute(NameOID.COMMON_NAME, f"LazyBoy Local CA ({hostname})"[:64])])
    now = _now()
    constraints = x509.NameConstraints(
        permitted_subtrees=[x509.DNSName("localhost"), x509.DNSName("local"), x509.DNSName(hostname),
                            *[x509.IPAddress(ipaddress.ip_network(n)) for n in PRIVATE_NETWORKS]],
        excluded_subtrees=None)
    cert = (x509.CertificateBuilder().subject_name(name).issuer_name(name).public_key(key.public_key())
            .serial_number(x509.random_serial_number()).not_valid_before(now - datetime.timedelta(hours=1))
            .not_valid_after(now + datetime.timedelta(days=3650))
            .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True)
            .add_extension(x509.KeyUsage(digital_signature=True, key_cert_sign=True, crl_sign=True, content_commitment=False,
                                         key_encipherment=False, data_encipherment=False, key_agreement=False,
                                         encipher_only=False, decipher_only=False), critical=True)
            .add_extension(constraints, critical=True)
            .add_extension(x509.SubjectKeyIdentifier.from_public_key(key.public_key()), critical=False)
            .sign(key, hashes.SHA256()))
    _write(key_path, _key_bytes(key), private=True)
    _write(cert_path, cert.public_bytes(serialization.Encoding.PEM))
    return cert, key


def _hostname_of(authority):
    """The host name the authority was made for: name constraints fix it for the authority's whole life."""
    constraints = authority.extensions.get_extension_for_class(x509.NameConstraints).value
    names = [n.value for n in constraints.permitted_subtrees or [] if isinstance(n, x509.DNSName)]
    return next((n for n in names if n not in {"localhost", "local"}), "localhost")


def _covers(cert, names, addresses):
    san = cert.extensions.get_extension_for_class(x509.SubjectAlternativeName).value
    return (set(san.get_values_for_type(x509.DNSName)) == set(names)
            and {str(a) for a in san.get_values_for_type(x509.IPAddress)} == {str(a) for a in addresses})


def ensure(directory: Path, hosts=None):
    """The server certificate and key files for this host, made or renewed as needed."""
    directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    authority, authority_key = _authority(directory, host_name())
    hostname = _hostname_of(authority)
    wanted = ["localhost", "127.0.0.1", "::1", *(public_hosts() if hosts is None else hosts)]
    skipped = [h for h in wanted if not _permitted(h, hostname)]
    if skipped: log.warning("HTTPS 憑證只涵蓋私人位址與本機名稱，略過：%s", ", ".join(skipped))
    wanted = list(dict.fromkeys(h for h in wanted if h not in skipped))
    names = [h for h in wanted if not _is_ip(h)]
    addresses = [ipaddress.ip_address(h) for h in wanted if _is_ip(h)]
    cert_path, key_path = directory / "server.crt", directory / "server.key"
    now = _now()
    with contextlib.suppress(OSError, ValueError, x509.ExtensionNotFound):
        current = x509.load_pem_x509_certificate(cert_path.read_bytes())
        if (key_path.exists() and current.issuer == authority.subject and current.not_valid_after_utc - now > RENEW_BEFORE
                and _covers(current, names, addresses)):
            return cert_path, key_path
    key = ec.generate_private_key(ec.SECP256R1())
    cert = (x509.CertificateBuilder()
            .subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, (names[1] if len(names) > 1 else "localhost")[:64])]))
            .issuer_name(authority.subject).public_key(key.public_key()).serial_number(x509.random_serial_number())
            .not_valid_before(now - datetime.timedelta(hours=1)).not_valid_after(now + datetime.timedelta(days=SERVER_DAYS))
            .add_extension(x509.SubjectAlternativeName([*map(x509.DNSName, names), *map(x509.IPAddress, addresses)]), critical=False)
            .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
            .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
            .add_extension(x509.KeyUsage(digital_signature=True, key_encipherment=False, content_commitment=False, data_encipherment=False,
                                         key_agreement=False, key_cert_sign=False, crl_sign=False, encipher_only=False,
                                         decipher_only=False), critical=True)
            .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(authority_key.public_key()), critical=False)
            .sign(authority_key, hashes.SHA256()))
    _write(key_path, _key_bytes(key), private=True)
    # The chain: server certificate, then the authority, so a device that trusts the authority accepts it.
    _write(cert_path, cert.public_bytes(serialization.Encoding.PEM) + authority.public_bytes(serialization.Encoding.PEM))
    log.info("已更新 HTTPS 憑證：%s", ", ".join(wanted))
    return cert_path, key_path


def fingerprint(directory: Path):
    """SHA-256 of the authority, for checking on a device that the right certificate was installed."""
    cert = x509.load_pem_x509_certificate((directory / "ca.crt").read_bytes())
    return cert.fingerprint(hashes.SHA256()).hex(":").upper()
