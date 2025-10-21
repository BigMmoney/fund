from typing import List, Optional, Dict, Any
from pydantic import BaseModel


class Subaccount(BaseModel):
    id: int
    name: str
    user_id: int


class SubaccountCreate(BaseModel):
    name: str
    user_id: int


class SubaccountUpdate(BaseModel):
    name: Optional[str] = None


class SubaccountService:
    """A simple in-memory subaccount service as a placeholder."""

    def __init__(self):
        self._db: Dict[int, Subaccount] = {}
        self._id_seq = 1

    async def create_subaccount(self, sub: SubaccountCreate) -> Subaccount:
        sid = self._id_seq
        self._id_seq += 1
        item = Subaccount(id=sid, name=sub.name, user_id=sub.user_id)
        self._db[sid] = item
        return item

    async def get_subaccounts(self, user_id: Optional[int] = None) -> List[Subaccount]:
        values = list(self._db.values())
        if user_id is not None:
            values = [v for v in values if v.user_id == user_id]
        return values

    async def get_subaccount(self, subaccount_id: int) -> Optional[Subaccount]:
        return self._db.get(subaccount_id)

    async def update_subaccount(self, subaccount_id: int, upd: SubaccountUpdate) -> Optional[Subaccount]:
        cur = self._db.get(subaccount_id)
        if not cur:
            return None
        if upd.name is not None:
            cur.name = upd.name
        self._db[subaccount_id] = cur
        return cur

    async def delete_subaccount(self, subaccount_id: int) -> bool:
        return self._db.pop(subaccount_id, None) is not None
