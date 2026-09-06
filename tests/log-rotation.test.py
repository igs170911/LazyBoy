import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('rotation', Path(__file__).resolve().parents[1] / 'image/computer/rotate-logs.py')
rotation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rotation)


class RotationTests(unittest.TestCase):
    def test_bounds_backups_and_keeps_live_writer(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            log = root / 'control.log'
            with log.open('ab', buffering=0) as writer:
                for n in range(6):
                    writer.write(bytes([65+n]) * 40)
                    rotation.rotate(root, 16)
                    self.assertEqual(log.stat().st_size, 0)
                    self.assertEqual(Path(f'{log}.1').read_bytes(), bytes([65+n])*16)
                writer.write(b'live')
                self.assertEqual(log.read_bytes(), b'live')
            self.assertEqual(len(list(root.glob('*.log.*'))), 3)
            rotation.rotate(root, 16)
            self.assertEqual(log.read_bytes(), b'live')

    def test_does_not_follow_symlinks_or_touch_other_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            target = root / 'notes.txt'
            target.write_bytes(b'important' * 20)
            (root / 'control.log').symlink_to(target)
            rotation.rotate(root, 16)
            self.assertEqual(target.read_bytes(), b'important' * 20)

if __name__ == '__main__':
    unittest.main()
