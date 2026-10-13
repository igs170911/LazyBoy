"""Every test runs against a throwaway data directory.

server.main opens its store, settings and access token at import time; without this, importing it from a test
opened (and migrated) the real .data/product.sqlite3 of the running service.
"""
import os
import tempfile

os.environ["LAZYHERMES_DATA_DIR"] = tempfile.mkdtemp(prefix="lazyboy-tests-")
