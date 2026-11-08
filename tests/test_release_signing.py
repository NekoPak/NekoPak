import base64
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / 'scripts/release_signing.py'

class ReleaseSignatureTests(unittest.TestCase):
    def test_sign_verify_reject_modified_bytes_and_version(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);private=root/'key';public=root/'public';artifact=root/'package.zip';manifest=root/'manifest.json'
            def run(*args):
                return subprocess.run([sys.executable,str(SCRIPT),*map(str,args)],capture_output=True,text=True)
            self.assertEqual(run('generate','--private',private,'--public',public).returncode,0)
            self.assertNotEqual(run('generate','--private',private,'--public',public).returncode,0)
            artifact.write_bytes(b'actual signed bytes')
            result=run('sign',artifact,'--version','0.1.0','--output',manifest,'--key-file',private)
            self.assertEqual(result.returncode,0,result.stderr)
            self.assertNotIn(private.read_text().strip(),result.stdout)
            self.assertEqual(run('verify','--manifest',manifest,'--public',public).returncode,0)
            payload=json.loads(manifest.read_text());payload['payload']['version']='9.9.9';manifest.write_text(json.dumps(payload))
            self.assertNotEqual(run('verify','--manifest',manifest,'--public',public).returncode,0)
            self.assertEqual(run('sign',artifact,'--version','0.1.0','--output',manifest,'--key-file',private).returncode,0)
            artifact.write_bytes(b'modified')
            self.assertNotEqual(run('verify','--manifest',manifest,'--public',public).returncode,0)

if __name__=='__main__':unittest.main()
