#!/bin/zsh
set -e

cd "$HOME/ezerpath"
source .venv/bin/activate
mkdir -p data/raw data/snapshots reports logs

# Clear the log for this run
> logs/scrapy.log

echo "[$(date)] Starting daily crawl..." >> logs/run.log

# Crawl all keywords from config/keywords.yaml
scrapy crawl onlinejobs 2>&1 | tee -a logs/run.log

# Generate the markdown report
python -m crawler.utils.markdown_report >> logs/run.log 2>&1

echo "[$(date)] Daily crawl complete." >> logs/run.log
