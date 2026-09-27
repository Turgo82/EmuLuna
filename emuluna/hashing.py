"""Streaming hashes compatible with every supported Python runtime."""
import hashlib


def file_hexdigest(stream, algorithm, chunk_size=1024 * 1024):
    """Hash a binary stream from its current position without loading it at once."""
    try:
        digest = hashlib.new(algorithm, usedforsecurity=False)
    except TypeError:  # Compatibility with Python/OpenSSL builds lacking the flag.
        digest = hashlib.new(algorithm)
    while True:
        chunk = stream.read(chunk_size)
        if not chunk:
            return digest.hexdigest()
        digest.update(chunk)
