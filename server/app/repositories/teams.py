"""
团队Repository
"""
from typing import List, Optional
from sqlalchemy.orm import Session
from app.models.team import Team
from server.app.base import BaseRepository

class TeamRepository(BaseRepository[Team]):
    """团队Repository"""
    
    def __init__(self, db: Session):
        super().__init__(Team, db)
    
    def create_team(self, name: str, created_by: int) -> Team:
        """创建团队"""
        team_data = {
            "name": name,
            "created_by": created_by
        }
        return self.create(team_data)
    
    def get_by_name(self, name: str) -> Optional[Team]:
        """根据名称获取团队"""
        return self.db.query(Team).filter(Team.name == name).first()
    
    def update_name(self, team: Team, name: str) -> Team:
        """更新团队名称"""
        return self.update(team, {"name": name})
    
    def get_teams_by_creator(self, created_by: int) -> List[Team]:
        """获取指定用户创建的团队"""
        return self.db.query(Team).filter(Team.created_by == created_by).all()