from fastapi import APIRouter, Query
from typing import List, Optional
from pydantic import BaseModel
import aiomysql
from ...db.mysql import get_pool

router = APIRouter()

class FlowItem(BaseModel):
    id: int
    user_id: int
    tx_hash: str
    action_type: str
    token_symbol: str
    amount: float
    usd_value: float
    txn_time: str
    settlement_time: str
    status: str

class FlowPage(BaseModel):
    total: int
    items: List[FlowItem]

@router.get("/api/flows", response_model=FlowPage)
async def flows(page: int = 1, page_size: int = 50, action_type: Optional[str] = None, user_id: Optional[int] = None):
    where = ["1=1"]
    params: list = []
    if action_type:
        where.append("action_type=%s")
        params.append(action_type)
    if user_id is not None:
        where.append("user_id=%s")
        params.append(user_id)
    where_sql = " AND ".join(where)
    offset = max(page - 1, 0) * page_size

    try:
        p = await get_pool()
        async with p.acquire() as conn:
            async with conn.cursor() as cur:
                await cur.execute(f"SELECT COUNT(*) FROM user_fund_flow WHERE {where_sql}", params)
                total = (await cur.fetchone())[0]
                await cur.execute(
                    f"""
                    SELECT id, user_id, tx_hash, action_type, token_symbol, amount, usd_value, txn_time, settlement_time, status
                    FROM user_fund_flow
                    WHERE {where_sql}
                    ORDER BY txn_time DESC
                    LIMIT %s OFFSET %s
                    """,
                    params + [page_size, offset]
                )
                cols = [c[0] for c in cur.description]
                rows = await cur.fetchall() or []
                items = [FlowItem(**dict(zip(cols, r))) for r in rows]
        return FlowPage(total=int(total), items=items)
    except Exception:
        return FlowPage(total=0, items=[])