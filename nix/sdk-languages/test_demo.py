# Two pytest tests the plugin marks, for checks.sdk-languages.
import rewind_sdk as rewind


def test_passes():
    assert rewind.always(True, "pytest-holds")


def test_fails():
    assert rewind.always(False, "pytest-fails", "pytest said so")
