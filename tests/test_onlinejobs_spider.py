import unittest
from datetime import datetime, timezone, timedelta

from crawler.spiders.onlinejobs import OnlineJobsSpider


class OnlineJobsSpiderTests(unittest.TestCase):
    def test_start_requests_uses_days_param(self):
        spider = OnlineJobsSpider(keyword="seo specialist", days="7")
        req = next(spider.start_requests())
        self.assertIn("dateposted=7", req.url)
        self.assertIn("jobkeyword=seo%20specialist", req.url)

    def test_days_input_is_validated_and_clamped(self):
        self.assertEqual(OnlineJobsSpider._normalize_days("abc"), 2)
        self.assertEqual(OnlineJobsSpider._normalize_days("-3"), 1)
        self.assertEqual(OnlineJobsSpider._normalize_days("999"), 30)

    def test_parse_date_returns_aware_utc_datetime(self):
        parsed = OnlineJobsSpider._parse_date("Posted on 2026-04-09 01:53:19")
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed.tzname(), "UTC")
        self.assertEqual(parsed.year, 2026)
        self.assertEqual(parsed.month, 4)
        self.assertEqual(parsed.day, 9)

    def test_parse_date_invalid_returns_none(self):
        self.assertIsNone(OnlineJobsSpider._parse_date("not a date"))

    def test_cutoff_tracks_days_window(self):
        spider = OnlineJobsSpider(keyword="seo specialist", days="1")
        delta = datetime.now(timezone.utc) - spider.cutoff
        self.assertGreater(delta, timedelta(hours=23))
        self.assertLess(delta, timedelta(hours=25))


if __name__ == "__main__":
    unittest.main()
