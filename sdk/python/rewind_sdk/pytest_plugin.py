# SPDX-License-Identifier: MIT
"""A pytest plugin that marks each test's start and result for Rewind.

Installed from the package it loads by itself; copied, load it with
`pytest -p rewind_sdk.pytest_plugin` or `pytest_plugins` in a conftest.
"""

import rewind_sdk

_RESULTS = {
    "passed": rewind_sdk.PASS,
    "failed": rewind_sdk.FAIL,
    "skipped": rewind_sdk.SKIP,
}


def pytest_runtest_logstart(nodeid, location):
    rewind_sdk.test_begin(nodeid)


def pytest_runtest_logreport(report):
    # A test ends with its call, or with its setup when that did not pass.
    if report.when == "call" or (report.when == "setup" and report.outcome != "passed"):
        rewind_sdk.test_end(
            report.nodeid, _RESULTS.get(report.outcome, rewind_sdk.FAIL)
        )
