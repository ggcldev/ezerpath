"""Upwork spider — PLACEHOLDER ONLY.

Do NOT wire this into the scheduled daily run until you confirm
an approved API or allowed automation path. Upwork's ToS restricts
bot/scraper access; use their official API instead.
"""

import scrapy


class UpworkSpider(scrapy.Spider):
    name = "upwork"
    allowed_domains = ["upwork.com"]

    def start_requests(self):
        self.logger.warning(
            "Upwork spider is a placeholder. "
            "Do not run until you have an approved API integration."
        )
        return []
