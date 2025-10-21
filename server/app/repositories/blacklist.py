"""
黑名单Repository
"""
from typing import List, Optional
from sqlalchemy.orm import Session
from app.models.blacklist import Blacklist
from server.app.base import BaseRepository

class BlacklistRepository(BaseRepository[Blacklist]):
    """黑名单Repository"""
    
    def __init__(self, db: Session):
        super().__init__(Blacklist, db)
    
    def create_blacklist_address(self, address: str, note: str) -> Blacklist:
        """创建黑名单地址"""
        # 地址转换为小写
        address = address.lower().strip()
        
        data = {
            "address": address,
            "note": note
        }
        return self.create(data)
    
    def get_by_address(self, address: str) -> Optional[Blacklist]:
        """根据地址获取黑名单记录"""
        address = address.lower().strip()
        return self.db.query(Blacklist).filter(Blacklist.address == address).first()
    
    def is_blacklisted(self, address: str) -> bool:
        """检查地址是否在黑名单中"""
        return self.get_by_address(address) is not None
    
    def get_all_addresses(self) -> List[str]:
        """获取所有黑名单地址"""
        blacklist_records = self.db.query(Blacklist.address).all()
        return [record.address for record in blacklist_records]