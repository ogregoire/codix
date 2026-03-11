from typing import List


class Repository:
    def save(self, item: "Item") -> None:
        pass

    def find(self, id: int) -> "Item":
        pass


class Item:
    name: str
    id: int


class Service:
    repo: Repository

    def __init__(self, repo: Repository):
        self.repo = repo

    def process(self) -> None:
        item = self.repo.find(1)
        self.repo.save(item)


@dataclass
class Config:
    debug: bool
