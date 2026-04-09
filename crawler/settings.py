BOT_NAME = "jobcrawler"

SPIDER_MODULES = ["crawler.spiders"]
NEWSPIDER_MODULE = "crawler.spiders"

# Respect robots.txt — OnlineJobs.ph specifies Crawl-delay: 5
ROBOTSTXT_OBEY = True
DOWNLOAD_DELAY = 5
CONCURRENT_REQUESTS_PER_DOMAIN = 1
CONCURRENT_REQUESTS = 2

AUTOTHROTTLE_ENABLED = True
AUTOTHROTTLE_START_DELAY = 5
AUTOTHROTTLE_MAX_DELAY = 30

RETRY_ENABLED = True
RETRY_TIMES = 2

# Enable the dedup pipeline
ITEM_PIPELINES = {
    "crawler.pipelines.DedupPipeline": 100,
}

FEEDS = {
    "data/raw/jobs-%(time)s.jsonl": {
        "format": "jsonlines",
        "encoding": "utf8",
        "overwrite": False,
    }
}

LOG_FILE = "logs/scrapy.log"
LOG_LEVEL = "INFO"

USER_AGENT = "jobcrawler/1.0 (+personal research crawler)"

REQUEST_FINGERPRINTER_IMPLEMENTATION = "2.7"
TWISTED_REACTOR = "twisted.internet.asyncioreactor.AsyncioSelectorReactor"
FEED_EXPORT_ENCODING = "utf-8"
