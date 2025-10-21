# Fund Management API

A comprehensive fund management system built with FastAPI, providing portfolio tracking, profit distribution, and user management capabilities.

## Features

- **User Authentication & Authorization**: JWT-based authentication with role-based permissions
- **Portfolio Management**: Track and manage investment portfolios
- **Team Management**: Organize users into teams for collaboration
- **Profit Distribution**: Manage profit allocation among team members
- **Blacklist Management**: Security features for wallet monitoring
- **Data Snapshots**: Historical data tracking and analysis
- **Ceffu API Integration**: Real-time data from Ceffu Prime Wallets

## Quick Start

### Prerequisites

- Python 3.8+
- MySQL database
- Virtual environment (recommended)

### Installation

1. **Clone the repository**
   ```bash
   cd server
   ```

2. **Create virtual environment**
   ```bash
   python -m venv venv
   # Windows
   venv\Scripts\activate
   # Linux/Mac
   source venv/bin/activate
   ```

3. **Install dependencies**
   ```bash
   pip install -r requirements.txt
   ```

4. **Configure environment**
   ```bash
   cp .env.example .env
   # Edit .env with your database credentials and API keys
   ```

5. **Set up database**
   ```bash
   # Create database in MySQL
   CREATE DATABASE fund_management;
   
   # Run migrations
   python migrate.py
   ```

6. **Start the application**
   ```bash
   python run.py
   ```

The API will be available at `http://localhost:8000`

## API Documentation

Once the application is running, you can access:

- **Interactive API docs**: http://localhost:8000/docs
- **ReDoc documentation**: http://localhost:8000/redoc

## Default Admin Account

After running migrations, a default admin account is created:

- **Email**: admin@example.com
- **Password**: admin123

⚠️ **Important**: Change the default password immediately after first login!

## Environment Configuration

Key environment variables in `.env`:

```env
# Database
DATABASE_URL=mysql+pymysql://username:password@localhost:3306/fund_management

# Security
SECRET_KEY=your-secret-key-here
ACCESS_TOKEN_EXPIRE_MINUTES=1440

# Ceffu API (from your OneToken discovery)
CEFFU_API_URL=https://open-api.ceffu.com
CEFFU_API_KEY=your-api-key
CEFFU_SECRET_KEY=your-secret-key

# Prime Wallet IDs (discovered from OneToken)
ZERODIVISION_BTC_WALLET_ID=540254796486533120
CI_USDT_ZEROD_BNB_WALLET_ID=542168616511160320
```

## API Endpoints

### Authentication
- `POST /auth/login` - User login
- `POST /auth/register` - Register new user (admin only)
- `GET /auth/me` - Get current user info
- `POST /auth/logout` - User logout

### User Management
- `GET /users` - List users with filtering
- `POST /users` - Create new user
- `GET /users/{id}` - Get user details
- `PUT /users/{id}` - Update user
- `DELETE /users/{id}` - Delete user

### Permissions
- `GET /auth/permissions` - List all permissions
- `POST /auth/users/{user_id}/permissions/{permission_id}` - Grant permission
- `DELETE /auth/users/{user_id}/permissions/{permission_id}` - Revoke permission

## Permission System

The system includes 5 permission types:

1. **user** - User management
2. **team** - Team management
3. **profit** - Profit allocation
4. **portfolio** - Portfolio management
5. **blacklist** - Security management

## Database Schema

The system includes comprehensive tables for:

- Users and permissions
- Teams and memberships
- Portfolios and allocations
- Profit management
- Data snapshots
- Blacklist management
- User sessions

## Development

### Project Structure

```
server/
├── app/
│   ├── main.py              # FastAPI application
│   ├── config.py            # Configuration management
│   ├── database.py          # Database connection
│   ├── models.py            # SQLAlchemy models
│   ├── schemas.py           # Pydantic schemas
│   ├── auth.py              # Authentication utilities
│   └── api/
│       ├── dependencies.py  # FastAPI dependencies
│       └── routers/
│           ├── auth.py      # Authentication endpoints
│           ├── users.py     # User management
│           └── health.py    # Health check
├── migrate.py               # Database migration
├── run.py                   # Application runner
├── requirements.txt         # Dependencies
└── .env                     # Environment variables
```

### Adding New Features

1. **Models**: Add SQLAlchemy models in `app/models.py`
2. **Schemas**: Add Pydantic schemas in `app/schemas.py`
3. **Routers**: Create new routers in `app/api/routers/`
4. **Dependencies**: Add permission checks in `app/api/dependencies.py`

### Testing

```bash
# Run with reload for development
python run.py

# Test authentication
curl -X POST "http://localhost:8000/auth/login" \
     -H "Content-Type: application/x-www-form-urlencoded" \
     -d "username=admin@example.com&password=admin123"
```

## Integration with Ceffu API

The system is configured to integrate with Ceffu Prime Wallets using the discovered wallet IDs:

- **zerodivision-btc**: 540254796486533120
- **CI-USDT-ZeroD-BNB**: 542168616511160320

Configure your Ceffu API credentials in the `.env` file to enable real-time data collection.

## Security Considerations

- Use strong SECRET_KEY in production
- Enable HTTPS in production
- Regularly rotate API keys
- Monitor authentication logs
- Implement rate limiting
- Use proper CORS configuration

## Support

For issues and questions:

1. Check the API documentation at `/docs`
2. Review the logs for detailed error information
3. Ensure all environment variables are properly configured
4. Verify database connectivity

## Next Steps

With the authentication system in place, you can now:

1. **Add Portfolio Management**: Implement portfolio CRUD operations
2. **Team Management**: Create team collaboration features  
3. **Profit Distribution**: Build profit allocation algorithms
4. **Data Collection**: Integrate with Ceffu API for real-time data
5. **Dashboard**: Create management dashboards
6. **Notifications**: Add alert and notification systems