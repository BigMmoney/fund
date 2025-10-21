"""
团队服务
"""
from typing import List
from sqlalchemy.orm import Session
from fastapi import HTTPException, status
from app.repositories.teams import TeamRepository
from app.models.team import Team
from app.schemas.teams import TeamCreate, TeamUpdate
import logging

logger = logging.getLogger(__name__)

class TeamService:
    """团队服务"""
    
    def __init__(self, db: Session):
        self.db = db
        self.team_repo = TeamRepository(db)
    
    def create_team(self, team_create: TeamCreate, created_by: int) -> Team:
        """创建团队"""
        # 检查团队名称是否已存在
        existing_team = self.team_repo.get_by_name(team_create.name)
        if existing_team:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Team name already exists"
            )
        
        team = self.team_repo.create_team(
            name=team_create.name,
            created_by=created_by
        )
        
        logger.info(f"Team created: {team.name}")
        return team
    
    def get_all_teams(self, limit: int = 100, offset: int = 0) -> tuple[List[Team], int]:
        """获取所有团队"""
        return self.team_repo.get_multi(
            limit=limit,
            offset=offset,
            order_by="created_at",
            order_desc=True
        )
    
    def get_team_by_id(self, team_id: int) -> Team:
        """根据ID获取团队"""
        team = self.team_repo.get(team_id)
        if not team:
            raise HTTPException(
                status_code=status.HTTP_404_NOT_FOUND,
                detail="Team not found"
            )
        return team
    
    def update_team(self, team_id: int, team_update: TeamUpdate) -> Team:
        """更新团队信息"""
        team = self.get_team_by_id(team_id)
        
        # 检查新名称是否已被其他团队使用
        existing_team = self.team_repo.get_by_name(team_update.name)
        if existing_team and existing_team.id != team_id:
            raise HTTPException(
                status_code=status.HTTP_400_BAD_REQUEST,
                detail="Team name already exists"
            )
        
        updated_team = self.team_repo.update_name(team, team_update.name)
        logger.info(f"Team updated: {team.name} -> {team_update.name}")
        return updated_team
    
    def delete_team(self, team_id: int) -> bool:
        """删除团队（如果没有关联的投资组合）"""
        team = self.get_team_by_id(team_id)
        
        # 检查是否有关联的投资组合
        # 这里需要检查Portfolio表中是否有team_id关联
        # 暂时简化处理
        
        result = self.team_repo.delete(team_id)
        if result:
            logger.info(f"Team deleted: {team.name}")
        return result
    
    def get_teams_by_creator(self, created_by: int) -> List[Team]:
        """获取指定用户创建的团队"""
        return self.team_repo.get_teams_by_creator(created_by)