import json
from pathlib import Path


class DedupPipeline:
    """Tracks previously seen job IDs across runs using a snapshot file.

    Jobs seen before get is_new=False; new jobs get is_new=True and are
    added to the snapshot for next time.
    """

    SNAPSHOT_PATH = Path("data/snapshots/seen_ids.jsonl")

    def open_spider(self, spider):
        self.seen = set()
        if self.SNAPSHOT_PATH.exists():
            for line in self.SNAPSHOT_PATH.read_text(encoding="utf-8").splitlines():
                if line.strip():
                    entry = json.loads(line)
                    self.seen.add(entry["id"])
        self.new_ids = []

    def process_item(self, item, spider):
        uid = f"{item['source']}:{item['source_id']}"
        if uid in self.seen:
            item["is_new"] = False
        else:
            item["is_new"] = True
            self.seen.add(uid)
            self.new_ids.append(uid)
        return item

    def close_spider(self, spider):
        if self.new_ids:
            self.SNAPSHOT_PATH.parent.mkdir(parents=True, exist_ok=True)
            with self.SNAPSHOT_PATH.open("a", encoding="utf-8") as f:
                for uid in self.new_ids:
                    f.write(json.dumps({"id": uid}) + "\n")
            spider.logger.info(
                f"Dedup: {len(self.new_ids)} new jobs, "
                f"{len(self.seen)} total tracked"
            )
