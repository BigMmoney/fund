"""
Team management router - 严格按照API需求文档实现
"""
from typing import Optional
from fastapi import APIRouter, Depends, Query
from sqlalchemy.orm import Session

from server.app.database import get_db
from server.app.schemas import TeamCreate, TeamUpdate
from server.app.models import Team, User
from server.app.api.dependencies import get_current_user, require_permission
from server.app.responses import StandardResponse, NotFoundError

router = APIRouter(prefix="/teams", tags=["Team Management"])


@router.get("")
async def get_teams(
    limit: int = Query(100, ge=1, le=1000, description="限制每次查询返回的size"),
    offset: int = Query(0, ge=0, description="搜索时指定的开始位置，从0开始"),
    db: Session = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    [37] GET /teams
    获取所有的交易团队信息
    """
    query = db.query(Team).order_by(Team.created_at.desc())
    
    # Get total count
    total = query.count()
    
    # Apply pagination
    teams = query.offset(offset).limit(limit).all()
    
    # Format team data according to API spec
    team_list = []
    for team in teams:
        team_list.append({
            "id": team.id,
            "name": team.name,
            "createdAt": int(team.created_at.timestamp()) if team.created_at else None,
            "createdBy": team.created_by
        })
    
    return StandardResponse.list_success(team_list, total)


@router.get("/{id}")
async def get_team(
    id: int,
    db: Session = Depends(get_db),
    current_user: User = Depends(get_current_user)
):
    """
    [38] GET /teams/{id}
    获取某个团队信息
    """
    team = db.query(Team).filter(Team.id == id).first()
    if not team:
        raise NotFoundError("Team not found")
    
    team_data = {
        "id": team.id,
        "name": team.name,
        "createdAt": int(team.created_at.timestamp()) if team.created_at else None,
        "createdBy": team.created_by
    }
    
    return StandardResponse.object_success(team_data)


@router.post("")
async def create_team(
    team_data: TeamCreate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_permission("team"))
):
    """
    [39] POST /teams
    添加新的交易团队
    """
    # Create new team
    new_team = Team(
        name=team_data.name,
        created_by=current_user.id
    )
    
    db.add(new_team)
    db.commit()
    db.refresh(new_team)
    
    team_data = {
        "id": new_team.id,
        "name": new_team.name,
        "createdAt": int(new_team.created_at.timestamp()) if new_team.created_at else None,
        "createdBy": new_team.created_by
    }
    
    return StandardResponse.object_success(team_data)


@router.put("/{id}")
async def update_team(
    id: int,
    team_data: TeamUpdate,
    db: Session = Depends(get_db),
    current_user: User = Depends(require_permission("team"))
):
    """
    [40] PUT /teams/{id}
    修改team的name
    """
    team = db.query(Team).filter(Team.id == id).first()
    if not team:
        raise NotFoundError("Team not found")
    
    # Update team name
    team.name = team_data.name
    
    db.commit()
    db.refresh(team)
    
    team_response = {
        "id": team.id,
        "name": team.name
    }
    
    return StandardResponse.object_success(team_response)