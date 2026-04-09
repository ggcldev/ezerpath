import scrapy


class JobItem(scrapy.Item):
    source = scrapy.Field()
    source_id = scrapy.Field()
    title = scrapy.Field()
    company = scrapy.Field()
    pay = scrapy.Field()
    posted_at = scrapy.Field()
    url = scrapy.Field()
    summary = scrapy.Field()
    keyword = scrapy.Field()
    scraped_at = scrapy.Field()
    is_new = scrapy.Field()
