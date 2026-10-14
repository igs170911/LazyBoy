"""HTTPS for other devices: one local authority per data directory, a server certificate for this host's names."""
import contextlib
import datetime
import socket
import ssl
import threading

import pytest
from cryptography import x509
from server import tls


@pytest.fixture(autouse=True)
def fixed_host(monkeypatch):
    monkeypatch.setenv("LAZYHERMES_HOSTNAME", "studio")
    monkeypatch.delenv("LAZYHERMES_PUBLIC_HOSTS", raising=False)


def san(path):
    cert = x509.load_pem_x509_certificate(path.read_bytes())
    names = cert.extensions.get_extension_for_class(x509.SubjectAlternativeName).value
    return set(names.get_values_for_type(x509.DNSName)) | {str(a) for a in names.get_values_for_type(x509.IPAddress)}


def test_server_certificate_covers_local_names_and_private_addresses_only(tmp_path):
    cert, key = tls.ensure(tmp_path, ["192.168.1.20", "studio", "studio.local", "8.8.8.8", "example.com"])
    assert san(cert) == {"localhost", "127.0.0.1", "::1", "192.168.1.20", "studio", "studio.local"}
    assert oct(key.stat().st_mode & 0o777) == "0o600" and oct((tmp_path / "ca.key").stat().st_mode & 0o777) == "0o600"
    authority = x509.load_pem_x509_certificate((tmp_path / "ca.crt").read_bytes())
    x509.load_pem_x509_certificate(cert.read_bytes()).verify_directly_issued_by(authority)
    assert "studio" in authority.subject.rfc4514_string()


def test_same_hosts_keep_the_certificate_and_a_new_address_keeps_the_authority(tmp_path):
    cert, _ = tls.ensure(tmp_path, ["192.168.1.20"])
    first, authority = cert.read_bytes(), (tmp_path / "ca.crt").read_bytes()
    tls.ensure(tmp_path, ["192.168.1.20"])
    assert cert.read_bytes() == first
    tls.ensure(tmp_path, ["192.168.1.21"])
    assert cert.read_bytes() != first and "192.168.1.21" in san(cert)
    assert (tmp_path / "ca.crt").read_bytes() == authority  # devices never trust again


def test_expiring_certificate_is_renewed(tmp_path, monkeypatch):
    cert, _ = tls.ensure(tmp_path, [])
    first = cert.read_bytes()
    later = datetime.datetime.now(datetime.UTC) + datetime.timedelta(days=tls.SERVER_DAYS - 10)
    monkeypatch.setattr(tls, "_now", lambda: later)
    tls.ensure(tmp_path, [])
    assert cert.read_bytes() != first


def test_docker_passes_the_hosts_addresses_and_name(monkeypatch):
    monkeypatch.setenv("LAZYHERMES_PUBLIC_HOSTS", "192.168.1.20, 100.101.1.2")
    assert tls.public_hosts() == ["192.168.1.20", "100.101.1.2", "studio", "studio.local"]


def handshake(cert, key, authority, name):
    """A real TLS connection, verified by OpenSSL the way a browser would (chain, host name, name constraints)."""
    server_context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER); server_context.load_cert_chain(cert, key)
    listener = socket.create_server(("127.0.0.1", 0)); port = listener.getsockname()[1]
    def serve():
        with listener, contextlib.suppress(OSError):
            connection, _ = listener.accept()
            with server_context.wrap_socket(connection, server_side=True) as secured: secured.recv(1)
    thread = threading.Thread(target=serve, daemon=True); thread.start()
    client = ssl.create_default_context(cafile=str(authority))
    try:
        with socket.create_connection(("127.0.0.1", port), timeout=5) as raw, client.wrap_socket(raw, server_hostname=name) as secured:
            secured.send(b"x")
    finally: thread.join(5)


def test_devices_that_trust_the_authority_accept_the_server(tmp_path):
    cert, key = tls.ensure(tmp_path, ["192.168.1.20", "studio.local"])
    for name in ("127.0.0.1", "localhost", "192.168.1.20", "studio.local"):
        handshake(cert, key, tmp_path / "ca.crt", name)
    with pytest.raises(ssl.SSLCertVerificationError):
        handshake(cert, key, tmp_path / "ca.crt", "192.168.1.99")


def test_a_leaked_authority_key_still_cannot_impersonate_a_public_website(tmp_path):
    from cryptography.hazmat.primitives import hashes, serialization
    from cryptography.hazmat.primitives.asymmetric import ec
    tls.ensure(tmp_path, [])
    authority = x509.load_pem_x509_certificate((tmp_path / "ca.crt").read_bytes())
    authority_key = serialization.load_pem_private_key((tmp_path / "ca.key").read_bytes(), None)
    key = ec.generate_private_key(ec.SECP256R1()); now = datetime.datetime.now(datetime.UTC)
    forged = (x509.CertificateBuilder().subject_name(x509.Name([])).issuer_name(authority.subject).public_key(key.public_key())
              .serial_number(1).not_valid_before(now - datetime.timedelta(hours=1)).not_valid_after(now + datetime.timedelta(days=1))
              .add_extension(x509.SubjectAlternativeName([x509.DNSName("bank.example.com")]), critical=True)
              .add_extension(x509.AuthorityKeyIdentifier.from_issuer_public_key(authority_key.public_key()), critical=False)
              .sign(authority_key, hashes.SHA256()))
    (tmp_path / "forged.crt").write_bytes(forged.public_bytes(serialization.Encoding.PEM))
    (tmp_path / "forged.key").write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
    with pytest.raises(ssl.SSLCertVerificationError, match="(?i)constraint|permitted|subtree"):
        handshake(tmp_path / "forged.crt", tmp_path / "forged.key", tmp_path / "ca.crt", "bank.example.com")
