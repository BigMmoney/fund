from fastapi import APIRouter

router = APIRouter()

# Here you can include the routers for different subaccounts
from server.app.api.routers import subaccounts, health, flows, r

router.include_router(health.router, prefix="/health", tags=["health"])
router.include_router(flows.router, prefix="/flows", tags=["flows"])
router.include_router(r.router, prefix="/r", tags=["calculations"])
router.include_router(subaccounts.router, prefix="/subaccounts", tags=["subaccounts"])