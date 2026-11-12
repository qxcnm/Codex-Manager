import base64
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "cm_image_gen.py"
SPEC = importlib.util.spec_from_file_location("cm_image_gen", SCRIPT)
imagegen = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(imagegen)
PNG_BYTES = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a2ioAAAAASUVORK5CYII="
)


class CredentialTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = patch.dict(os.environ, {"CODEX_HOME": str(self.root)}, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)

    def configure(self, provider_fields="", root_fields="", auth=None):
        (self.root / "config.toml").write_text(
            'model_provider = "cm"\n' + root_fields
            + '\n[model_providers.cm]\nbase_url = "http://127.0.0.1:48760/v1"\n'
            + provider_fields,
            encoding="utf-8",
        )
        if auth is not None:
            (self.root / "auth.json").write_text(json.dumps(auth), encoding="utf-8")

    def test_provider_key_precedes_unrelated_auth_json_key(self):
        self.configure('experimental_bearer_token = "platform-key"', auth={"OPENAI_API_KEY": "other-key"})
        self.assertEqual(imagegen.load_api_key(), "platform-key")

    def test_python_310_simple_toml_fallback_reads_provider_key(self):
        self.configure('experimental_bearer_token = "platform-key"', auth={"OPENAI_API_KEY": None})
        with patch.object(imagegen, "tomllib", None):
            self.assertEqual(imagegen.load_api_key(), "platform-key")

    def test_legacy_root_bearer_token(self):
        self.configure(root_fields='experimental_bearer_token = "legacy-platform-key"\n')
        self.assertEqual(imagegen.load_api_key(), "legacy-platform-key")

    def test_provider_env_key(self):
        self.configure('env_key = "TEST_PROVIDER_KEY"',
                       root_fields='experimental_bearer_token = "stale-root-key"\n',
                       auth={"OPENAI_API_KEY": "other-key"})
        os.environ["TEST_PROVIDER_KEY"] = "provider-env-key"
        self.assertEqual(imagegen.load_api_key(), "provider-env-key")

    def test_selected_profile_controls_both_endpoint_and_credential(self):
        self.configure(
            'experimental_bearer_token = "inactive-root-key"\n'
            '[profiles.images]\nmodel_provider = "image-provider"\n'
            '[model_providers.image-provider]\nbase_url = "http://127.0.0.1:12345/v1"\n'
            'env_key = "TEST_IMAGE_PROVIDER_KEY"\n', root_fields='profile = "images"\n'
        )
        os.environ["TEST_IMAGE_PROVIDER_KEY"] = "selected-profile-key"
        self.assertEqual(imagegen.provider_base_url(), "http://127.0.0.1:12345/v1")
        self.assertEqual(imagegen.load_api_key(), "selected-profile-key")

    def test_profile_without_provider_inherits_root_provider(self):
        self.configure('experimental_bearer_token = "root-provider-key"\n'
                       '[profiles.images]\nmodel = "unused-text-model"\n', root_fields='profile = "images"\n')
        self.assertEqual(imagegen.load_api_key(), "root-provider-key")

    def test_missing_selected_profile_fails_instead_of_using_root_provider(self):
        self.configure('experimental_bearer_token = "root-provider-key"', root_fields='profile = "missing"\n')
        with self.assertRaisesRegex(SystemExit, "Profile 'missing' is not defined"):
            imagegen.load_api_key()

    def test_missing_provider_env_key_does_not_use_unrelated_auth(self):
        self.configure('env_key = "TEST_PROVIDER_KEY"',
                       root_fields='experimental_bearer_token = "stale-root-key"\n',
                       auth={"OPENAI_API_KEY": "other-key"})
        with self.assertRaisesRegex(SystemExit, "TEST_PROVIDER_KEY.*not set"):
            imagegen.load_api_key()

    def test_undefined_provider_does_not_use_legacy_key_with_base_url_override(self):
        (self.root / "config.toml").write_text('model_provider = "missing"\n'
                                             'experimental_bearer_token = "stale-root-key"\n', encoding="utf-8")
        os.environ["CODEXMANAGER_IMAGE_BASE_URL"] = "http://127.0.0.1:12345/v1"
        with self.assertRaisesRegex(SystemExit, "model_provider 'missing' is not defined"):
            imagegen.load_api_key()

    def test_legacy_auth_json_key(self):
        self.configure(auth={"auth_mode": "apikey", "OPENAI_API_KEY": "legacy-key"})
        self.assertEqual(imagegen.load_api_key(), "legacy-key")

    def test_chatgpt_token_is_not_a_platform_key(self):
        self.configure(auth={"auth_mode": "chatgpt", "OPENAI_API_KEY": None,
                             "tokens": {"access_token": "private-chatgpt-token"}})
        with self.assertRaises(SystemExit) as failure:
            imagegen.load_api_key()
        self.assertIn("experimental_bearer_token", str(failure.exception))
        self.assertNotIn("private-chatgpt-token", str(failure.exception))

    def test_inactive_provider_key_is_ignored(self):
        self.configure('\n[model_providers.other]\nexperimental_bearer_token = "other-key"')
        with self.assertRaisesRegex(SystemExit, "No image API credential"):
            imagegen.load_api_key()

    def test_invalid_auth_json_does_not_echo_contents(self):
        self.configure()
        (self.root / "auth.json").write_text('{"OPENAI_API_KEY": "secret-contents"', encoding="utf-8")
        with self.assertRaises(SystemExit) as failure:
            imagegen.load_api_key()
        self.assertIn("Failed to read authentication", str(failure.exception))
        self.assertNotIn("secret-contents", str(failure.exception))

    @unittest.skipIf(imagegen.tomllib is None, "Quoted TOML tables require Python 3.11+")
    def test_quoted_provider_and_hash_in_token(self):
        (self.root / "config.toml").write_text(
            'model_provider = "my.provider"\n[model_providers."my.provider"]\n'
            'base_url = "http://127.0.0.1:48760/v1"\n'
            'experimental_bearer_token = "platform # token"\n', encoding="utf-8"
        )
        self.assertEqual(imagegen.provider_base_url(), "http://127.0.0.1:48760/v1")
        self.assertEqual(imagegen.load_api_key(), "platform # token")


class LocalGatewayTests(unittest.TestCase):
    def test_generate_and_edit_use_provider_key_without_rewriting_auth(self):
        requests = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                payload = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                requests.append((self.path, self.headers.get("Authorization"), payload))
                response = json.dumps({"created": 1, "data": [{
                    "b64_json": base64.b64encode(PNG_BYTES).decode("ascii")
                }]}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(response)))
                self.end_headers()
                self.wfile.write(response)

            def log_message(self, *args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                config_path = root / "config.toml"
                config_path.write_text(
                    'model_provider = "cm"\n[model_providers.cm]\n'
                    f'base_url = "http://127.0.0.1:{server.server_port}/v1"\n'
                    'experimental_bearer_token = "mock-platform-key"\n', encoding="utf-8"
                )
                auth_path = root / "auth.json"
                auth_path.write_text('{"auth_mode":"chatgpt","OPENAI_API_KEY":null, '
                                     '"tokens":{"access_token":"preserved-login"}}', encoding="utf-8")
                original_config = config_path.read_bytes()
                original_auth = auth_path.read_bytes()
                source = root / "source.png"
                source.write_bytes(PNG_BYTES)
                env = dict(os.environ, CODEX_HOME=str(root))
                for name in ("CODEXMANAGER_IMAGE_BASE_URL", "CODEXMANAGER_IMAGE_MODEL"):
                    env.pop(name, None)
                for operation in ("generate", "edit"):
                    args = [sys.executable, str(SCRIPT), operation, "--prompt", "a small cat",
                            "--out-dir", str(root / "output"), "--filename", operation + ".png"]
                    if operation == "edit":
                        # The gateway must also work without an auth file.
                        auth_path.unlink()
                        args.extend(["--image", str(source)])
                    run = subprocess.run(args, env=env, capture_output=True, text=True, timeout=15)
                    self.assertEqual(run.returncode, 0, run.stderr)
                    result = json.loads(run.stdout)
                    self.assertEqual(Path(result["paths"][0]).read_bytes(), PNG_BYTES)
                    self.assertNotIn("mock-platform-key", run.stdout + run.stderr)
                    self.assertEqual(config_path.read_bytes(), original_config)
                    if operation == "generate":
                        self.assertEqual(auth_path.read_bytes(), original_auth)
                    else:
                        self.assertFalse(auth_path.exists())
                self.assertEqual([r[0] for r in requests], ["/v1/images/generations", "/v1/images/edits"])
                for _, authorization, payload in requests:
                    self.assertEqual(authorization, "Bearer mock-platform-key")
                    self.assertEqual(payload["model"], "gpt-image-2")
                self.assertTrue(requests[1][2]["images"][0]["image_url"].startswith("data:image/png;base64,"))
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)


if __name__ == "__main__":
    unittest.main()
