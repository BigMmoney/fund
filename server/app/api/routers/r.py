from fastapi import APIRouter, Depends, HTTPException
from typing import List
from server.app.dependencies import get_current_user
from ...services.subaccounts import SubaccountService, Subaccount, SubaccountCreate, SubaccountUpdate

router = APIRouter()
service = SubaccountService()

@router.post("/subaccounts/", response_model=Subaccount)
async def create_subaccount(subaccount: SubaccountCreate, current_user: dict = Depends(get_current_user)):
    return await service.create_subaccount(subaccount)

@router.get("/subaccounts/", response_model=List[Subaccount])
async def list_subaccounts(current_user: dict = Depends(get_current_user)):
    return await service.get_subaccounts()

@router.get("/subaccounts/{subaccount_id}", response_model=Subaccount)
async def get_subaccount(subaccount_id: int, current_user: dict = Depends(get_current_user)):
    result = await service.get_subaccount(subaccount_id)
    if not result:
        raise HTTPException(status_code=404, detail="Subaccount not found")
    return result

@router.put("/subaccounts/{subaccount_id}", response_model=Subaccount)
async def update_subaccount(subaccount_id: int, subaccount: SubaccountUpdate, current_user: dict = Depends(get_current_user)):
    result = await service.update_subaccount(subaccount_id, subaccount)
    if not result:
        raise HTTPException(status_code=404, detail="Subaccount not found")
    return result

@router.delete("/subaccounts/{subaccount_id}", response_model=dict)
async def delete_subaccount(subaccount_id: int, current_user: dict = Depends(get_current_user)):
    success = await service.delete_subaccount(subaccount_id)
    if not success:
        raise HTTPException(status_code=404, detail="Subaccount not found")
    return {"ok": True}