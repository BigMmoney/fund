# Database utilities
try:
    from server.app.db.mysql import get_pool, close_pool, execute_query, execute_many
except ImportError:
    # Fallback for different import paths
    try:
        from app.db.mysql import get_pool, close_pool, execute_query, execute_many
    except ImportError:
        pass  # Will be handled at runtime

__all__ = ["get_pool", "close_pool", "execute_query", "execute_many"]