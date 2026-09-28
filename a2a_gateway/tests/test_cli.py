"""Gateway listener settings retain an explicit network HTTP opt-in."""

import unittest

from nl2sh_a2a.__main__ import validate_listener


class ListenerTests(unittest.TestCase):
    def test_loopback_and_https_network(self):
        validate_listener("127.0.0.1", "http://127.0.0.1:8765", False)
        validate_listener("127.0.0.1", "https://gateway.example", False)
        with self.assertRaisesRegex(ValueError, "network bind requires"):
            validate_listener("0.0.0.0", "https://gateway.example", False)

    def test_network_http_requires_opt_in(self):
        with self.assertRaisesRegex(ValueError, "HTTPS or explicit"):
            validate_listener("0.0.0.0", "http://192.168.1.10:8765", False)
        validate_listener("0.0.0.0", "http://192.168.1.10:8765", True)

    def test_advertised_url_rejects_credentials_and_path(self):
        for url in ("http://user:secret@192.168.1.10:8765", "https://gateway.example/a2a"):
            with self.assertRaises(ValueError):
                validate_listener("0.0.0.0", url, True)


if __name__ == "__main__":
    unittest.main()
