# Mocked connection-loss and typed-outcome cases for remote-test-test.py.
# Mixed into its command fixture; dependencies: json and the fixture's helpers.
import json


class ConnectionCases:
    def test_dropped_connection_preserves_status_and_names_inspection(self) -> None:
        for output in ["", "test result: ok. 1 passed; 0 failed; 0 ignored;\n"]:
            with self.subTest(output=output):
                self.env.update(FAKE_STATUS="255", FAKE_TEST_OUTPUT=output)
                result = self.run_script()
                self.assertEqual(result.returncode, 255)
                self.assertIn("job fixture-job: rbox exited 255 after the job started; "
                              "the job may continue on the box", result.stderr)
                self.assertIn("rbox log fixture fixture-job", result.stderr)
                self.assertEqual("tests had started" in result.stderr, bool(output))
                self.assertNotIn("failed before any test ran", result.stderr)

    def test_typed_outcomes_override_text_when_decisive(self) -> None:
        cases = [
            ({"phase": "unknown", "job": "typed-job"}, "no", "no", "after the job started"),
            ({"phase": "completed", "job": "typed-job"}, "no", "no", "tests failed"),
            ({"phase": "timed_out", "job": "typed-job"}, "no", "yes", "timed out"),
            ({"phase": "not_started", "job": None}, "no", "no", "failed before any test ran"),
            ({"phase": "unknown", "job": None}, "yes", "no", "after the job started"),
            ({"phase": "future"}, "yes", "no", "after the job started"),
        ]
        for receipt, job, complete, message in cases:
            with self.subTest(receipt=receipt):
                self.env.update(AID_RBOX_RESULT=str(self.base / "results.jsonl"),
                                FAKE_RECEIPT=json.dumps({"version": 1, **receipt}),
                                FAKE_STATUS="255", FAKE_JOB=job, FAKE_COMPLETE=complete)
                result = self.run_script()
                self.assertEqual(result.returncode, 255)
                self.assertIn(message, result.stderr)

    def test_not_started_receipt_preserves_log_observed_job(self) -> None:
        for complete in ["no", "yes"]:
            with self.subTest(complete=complete):
                self.env.update(AID_RBOX_RESULT=str(self.base / "results.jsonl"),
                                FAKE_RECEIPT=json.dumps({"version": 1, "phase": "not_started",
                                                        "job": None}),
                                FAKE_STATUS="255", FAKE_COMPLETE=complete)
                result = self.run_script()
                self.assertEqual(result.returncode, 255)
                self.assertIn("job fixture-job:", result.stderr)
                self.assertNotIn("failed before any test ran", result.stderr)
                if complete == "no":
                    self.assertIn("rbox exited 255 after the job started", result.stderr)
                    self.assertIn("rbox log fixture fixture-job", result.stderr)
                else:
                    self.assertIn("tests failed (exit 255)", result.stderr)

    def test_uncertain_launch_without_job_id_never_claims_no_tests_ran(self) -> None:
        for phase in ["unknown", "future_phase"]:
            for status in [1, 255]:
                with self.subTest(phase=phase, status=status):
                    receipt = {"version": 1, "phase": phase, "job": None,
                               "reason": "transport_lost"}
                    self.env.update(AID_RBOX_RESULT=str(self.base / "results.jsonl"),
                                    FAKE_RECEIPT=json.dumps(receipt), FAKE_JOB="no",
                                    FAKE_STATUS=str(status))
                    result = self.run_script()
                    self.assertEqual(result.returncode, status)
                    self.assertIn(f"job unavailable: rbox exited {status} after the job started; "
                                  "the job may continue on the box", result.stderr)
                    self.assertNotIn("failed before any test ran", result.stderr)
                    self.assertNotIn("rbox log", result.stderr)

    def test_unusable_receipts_keep_text_classification(self) -> None:
        for receipt in ["invalid", "[]", '{"version":2,"phase":"not_started"}']:
            with self.subTest(receipt=receipt):
                self.env.update(AID_RBOX_RESULT=str(self.base / "results.jsonl"),
                                FAKE_RECEIPT=receipt, FAKE_STATUS="255")
                result = self.run_script()
                self.assertEqual(result.returncode, 255)
                self.assertIn("after the job started", result.stderr)

    def test_reserved_statuses_do_not_use_connection_loss_diagnostic(self) -> None:
        for status in [0, 66, 69, 75, 124]:
            with self.subTest(status=status):
                self.env["FAKE_STATUS"] = str(status)
                result = self.run_script()
                self.assertEqual(result.returncode, status)
                self.assertNotIn("after the job started", result.stderr)
                self.assertNotIn("failed before any test ran", result.stderr)
