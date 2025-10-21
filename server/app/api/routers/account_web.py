sfrom fastapi import APIRouter, Request, Form
from fastapi.responses import HTMLResponse, RedirectResponse
from fastapi.templating import Jinja2Templates
from pathlib import Path
from ...services.account_manager import APIAccountManager

router = APIRouter()
# templates 目录位于 app/templates 下
_app_root = Path(__file__).resolve().parents[2]
templates = Jinja2Templates(directory=_app_root / "templates")
# 账号 YAML 位于 app/services/api_accounts.yaml 下
account_manager = APIAccountManager(str(_app_root / "services" / "api_accounts.yaml"))

@router.get("/accounts", response_class=HTMLResponse)
def get_accounts(request: Request):
    accounts = account_manager.get_all_accounts()
    return templates.TemplateResponse("accounts.html", {"request": request, "accounts": accounts})

@router.post("/accounts/add", response_class=HTMLResponse)
def add_account(request: Request, vendor: str = Form(...), name: str = Form(...), api_key: str = Form(...), api_secret: str = Form(...)):
    account_manager.add_account(vendor, {"name": name, "api_key": api_key, "api_secret": api_secret, "base_url": ""})
    return RedirectResponse(url="/accounts", status_code=303)

@router.post("/accounts/delete", response_class=HTMLResponse)
def delete_account(request: Request, vendor: str = Form(...), name: str = Form(...)):
    account_manager.remove_account(vendor, name)
    return RedirectResponse(url="/accounts", status_code=303)
