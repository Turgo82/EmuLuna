"""Compatibility checks for streaming file hashes used by packaged builds."""
import hashlib
import io
import unittest

from emuluna.hashing import file_hexdigest


class FileHashing(unittest.TestCase):
    def test_hashes_large_stream_in_chunks(self):
        payload = bytes(range(256)) * 10000
        stream = io.BytesIO(payload)
        self.assertEqual(file_hexdigest(stream, 'sha256', 4093),
                         hashlib.sha256(payload).hexdigest())
        self.assertEqual(stream.tell(), len(payload))

    def test_hashes_from_current_stream_position(self):
        stream = io.BytesIO(b'ignored' + b'game data')
        stream.seek(len(b'ignored'))
        self.assertEqual(file_hexdigest(stream, 'md5', 3),
                         hashlib.md5(b'game data', usedforsecurity=False).hexdigest())
