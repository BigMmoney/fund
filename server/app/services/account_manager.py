import yaml
from pathlib import Path
from typing import List, Dict, Any

class APIAccountManager:
    def __init__(self, yaml_path: str):
        self.yaml_path = Path(yaml_path)
        self.data = self._load()

    def _load(self) -> Dict[str, Any]:
        if self.yaml_path.exists():
            with open(self.yaml_path, 'r', encoding='utf-8') as f:
                return yaml.safe_load(f) or {}
        return {"ceffu": [], "1token": []}

    def save(self):
        with open(self.yaml_path, 'w', encoding='utf-8') as f:
            yaml.safe_dump(self.data, f, allow_unicode=True)

    def add_account(self, vendor: str, account: Dict[str, Any]):
        if vendor not in self.data:
            self.data[vendor] = []
        self.data[vendor].append(account)
        self.save()

    def remove_account(self, vendor: str, name: str):
        if vendor in self.data:
            self.data[vendor] = [a for a in self.data[vendor] if a.get('name') != name]
            self.save()

    def get_accounts(self, vendor: str) -> List[Dict[str, Any]]:
        return self.data.get(vendor, [])

    def get_all_accounts(self) -> Dict[str, List[Dict[str, Any]]]:
        return self.data
