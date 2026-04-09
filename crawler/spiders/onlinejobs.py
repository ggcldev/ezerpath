import re
import scrapy
import yaml
from datetime import datetime, timezone, timedelta
from pathlib import Path
from urllib.parse import quote

from crawler.items import JobItem


class OnlineJobsSpider(scrapy.Spider):
    name = "onlinejobs"
    allowed_domains = ["onlinejobs.ph"]

    # Max search result pages to crawl per keyword (30 jobs/page)
    MAX_PAGES_PER_KEYWORD = 5
    DEFAULT_DAYS = 2
    MIN_DAYS = 1
    MAX_DAYS = 30

    def __init__(self, keyword=None, days="2", *args, **kwargs):
        super().__init__(*args, **kwargs)
        self.days = self._normalize_days(days)
        self.max_age = timedelta(days=self.days)
        self.cutoff = datetime.now(timezone.utc) - self.max_age

        if keyword:
            self.keywords = [keyword]
        else:
            config_path = Path("config/keywords.yaml")
            if config_path.exists():
                with open(config_path, encoding="utf-8") as f:
                    cfg = yaml.safe_load(f)
                self.keywords = cfg.get("keywords", ["seo specialist"])
            else:
                self.keywords = ["seo specialist"]

    def start_requests(self):
        for kw in self.keywords:
            q = quote(kw)
            url = (
                f"https://www.onlinejobs.ph/jobseekers/jobsearch"
                f"?jobkeyword={q}&dateposted={self.days}"
            )
            yield scrapy.Request(
                url,
                callback=self.parse_search,
                cb_kwargs={"keyword": kw, "page": 1},
            )

    @classmethod
    def _normalize_days(cls, raw_days):
        """Normalize user input into an integer days range accepted by the spider."""
        try:
            days = int(raw_days)
        except (TypeError, ValueError):
            return cls.DEFAULT_DAYS
        if days < cls.MIN_DAYS:
            return cls.MIN_DAYS
        if days > cls.MAX_DAYS:
            return cls.MAX_DAYS
        return days

    def parse_search(self, response, keyword, page=1):
        cards = response.css(".jobpost-cat-box")
        self.logger.info(
            f"[{keyword}] Page {page}: {len(cards)} cards"
        )

        found_old = False
        for card in cards:
            # Date from card: <em>Posted on YYYY-MM-DD HH:MM:SS</em>
            date_text = card.css("p.fs-13 em::text").get(default="")
            posted_dt = self._parse_date(date_text)

            if posted_dt and posted_dt < self.cutoff:
                found_old = True
                continue

            # Extract data directly from the search card (no detail page visit)
            title = card.css("h4::text").get(default="").strip()
            company = card.css(".jobpost-cat-box-logo::attr(alt)").get(default="").strip()
            desc = card.css(".desc::text").getall()
            summary = " ".join(t.strip() for t in desc if t.strip())

            # Get the job link for the URL and source_id
            link = None
            for href in card.css("a::attr(href)").getall():
                if "/jobseekers/job/" in href:
                    link = href
                    break

            if not link:
                continue

            match = re.search(r"(\d+)$", link.rstrip("/"))
            source_id = match.group(1) if match else link.split("/")[-1]

            full_url = response.urljoin(link)

            item = JobItem()
            item["source"] = "onlinejobs"
            item["source_id"] = source_id
            item["title"] = title
            item["company"] = company
            item["pay"] = ""
            item["posted_at"] = posted_dt.isoformat() if posted_dt else ""
            item["url"] = full_url
            item["summary"] = summary[:500]
            item["keyword"] = keyword
            item["scraped_at"] = datetime.now(timezone.utc).isoformat()
            item["is_new"] = True
            yield item

        # Paginate: stop if we found old posts or hit page limit
        if not found_old and page < self.MAX_PAGES_PER_KEYWORD:
            next_page = response.css('a[rel="next"]::attr(href)').get()
            if next_page:
                yield response.follow(
                    next_page,
                    callback=self.parse_search,
                    cb_kwargs={"keyword": keyword, "page": page + 1},
                )

    @staticmethod
    def _parse_date(text):
        """Parse 'Posted on 2026-04-09 01:53:19' into a timezone-aware datetime."""
        match = re.search(r"(\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2})", text)
        if match:
            dt = datetime.strptime(match.group(1), "%Y-%m-%d %H:%M:%S")
            return dt.replace(tzinfo=timezone.utc)
        return None
