"""
Database migration script to set up initial database
"""
import os
import sys
from datetime import datetime

# Add the parent directory to the path so we can import our app
sys.path.append(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from app.database import engine, SessionLocal
from app.models import Base, User, Permission, UserPermission
from app.auth import get_password_hash
from app.config import settings

def create_tables():
    """Create all database tables"""
    print("Creating database tables...")
    Base.metadata.create_all(bind=engine)
    print("Database tables created successfully!")

def create_permissions():
    """Create default permissions"""
    db = SessionLocal()
    try:
        print("Creating default permissions...")
        
        # Check if permissions already exist
        existing_permissions = db.query(Permission).count()
        if existing_permissions > 0:
            print("Permissions already exist, skipping...")
            return
        
        # Create default permissions
        permissions = [
            Permission(
                id="user",
                name="User Management",
                description="Manage users and their accounts"
            ),
            Permission(
                id="team",
                name="Team Management", 
                description="Manage teams and team memberships"
            ),
            Permission(
                id="profit",
                name="Profit Management",
                description="Manage profit allocation and distribution"
            ),
            Permission(
                id="portfolio",
                name="Portfolio Management",
                description="Manage portfolios and investments"
            ),
            Permission(
                id="blacklist",
                name="Blacklist Management",
                description="Manage wallet blacklist and security"
            )
        ]
        
        for permission in permissions:
            db.add(permission)
        
        db.commit()
        print("Default permissions created successfully!")
        
    except Exception as e:
        print(f"Error creating permissions: {e}")
        db.rollback()
    finally:
        db.close()

def create_admin_user():
    """Create default admin user"""
    db = SessionLocal()
    try:
        print("Creating default admin user...")
        
        # Check if admin user already exists
        existing_admin = db.query(User).filter(User.email == "admin@example.com").first()
        if existing_admin:
            print("Admin user already exists, skipping...")
            return
        
        # Create admin user
        admin_password = "admin123"  # Change this in production!
        admin_user = User(
            email="admin@example.com",
            password_hash=get_password_hash(admin_password),
            name="System Administrator",
            is_super=True,
            is_active=True
        )
        
        db.add(admin_user)
        db.commit()
        db.refresh(admin_user)
        
        # Grant all permissions to admin
        permissions = db.query(Permission).all()
        for permission in permissions:
            user_permission = UserPermission(
                user_id=admin_user.id,
                permission_id=permission.id
            )
            db.add(user_permission)
        
        db.commit()
        
        print("Default admin user created successfully!")
        print(f"Email: admin@example.com")
        print(f"Password: {admin_password}")
        print("Please change the password after first login!")
        
    except Exception as e:
        print(f"Error creating admin user: {e}")
        db.rollback()
    finally:
        db.close()

def main():
    """Main migration function"""
    print("Starting database migration...")
    print(f"Database URL: {settings.database_url}")
    
    try:
        # Create tables
        create_tables()
        
        # Create permissions
        create_permissions()
        
        # Create admin user
        create_admin_user()
        
        print("Database migration completed successfully!")
        
    except Exception as e:
        print(f"Migration failed: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()