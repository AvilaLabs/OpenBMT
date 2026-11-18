import unittest

from authority_acceptance import AcceptanceFailure, Device


class DownloadFileSizeTest(unittest.TestCase):
    @staticmethod
    def fake_device(listing, stat_result=None, listing_error=None, stat_error=None):
        device = Device.__new__(Device)

        def shell(*args):
            if args == ("ls", "-1", "/sdcard/Download"):
                if listing_error is not None:
                    raise listing_error
                return listing
            if args[:3] == ("stat", "-c", "%s"):
                if stat_error is not None:
                    raise stat_error
                return stat_result
            raise AssertionError(f"unexpected shell command: {args[0]}")

        device.shell = shell
        return device

    def test_absent_name_is_reported_as_missing(self):
        device = self.fake_device("other.csv\n", stat_result="12\n")
        self.assertIsNone(device.download_file_size("interrupted.csv"))

    def test_listed_file_size_is_returned(self):
        device = self.fake_device("export.csv\n", stat_result="123\n")
        self.assertEqual(device.download_file_size("export.csv"), 123)

    def test_directory_or_stat_inspection_errors_fail(self):
        listing_failure = AcceptanceFailure("adb operation failed: shell")
        with self.assertRaises(AcceptanceFailure):
            self.fake_device("", listing_error=listing_failure).download_file_size("export.csv")

        stat_failure = AcceptanceFailure("adb operation failed: shell")
        with self.assertRaises(AcceptanceFailure):
            self.fake_device("export.csv\n", stat_error=stat_failure).download_file_size("export.csv")


if __name__ == "__main__":
    unittest.main()
