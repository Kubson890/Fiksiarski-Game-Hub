import os
import sys
import sqlite3
import hashlib
import secrets
import json
import html
from contextlib import contextmanager

# Zgodność wersji python-multipart na różnych dystrybucjach (np. Arch/CachyOS vs Debian/Docker)
try:
    import multipart
    if "multipart.multipart" not in sys.modules:
        sys.modules["multipart.multipart"] = multipart
except ImportError:
    pass
try:
    import bcrypt
    HAS_BCRYPT = True
except ImportError:
    HAS_BCRYPT = False
from fastapi import FastAPI, HTTPException, Form, UploadFile, File, WebSocket, WebSocketDisconnect, Depends, status
from fastapi.responses import HTMLResponse, RedirectResponse, Response, FileResponse
from fastapi.security import HTTPBasic, HTTPBasicCredentials
from pydantic import BaseModel
from typing import Dict
import uvicorn

app = FastAPI(title="Fiksiarski Game Hub Server")
security = HTTPBasic()

SERVER_PORT = int(os.environ.get("SERVER_PORT", "47832"))
ADMIN_USER = os.environ.get("ADMIN_USER", "admin")
ADMIN_PASSWORD = os.environ.get("ADMIN_PASSWORD", "xK9$mRv!47Qz")

DATA_DIR = os.environ.get("DATA_DIR", ".")
os.makedirs(DATA_DIR, exist_ok=True)
RELEASES_DIR = os.path.join(DATA_DIR, "releases")
os.makedirs(RELEASES_DIR, exist_ok=True)

DB_PATH = os.path.join(DATA_DIR, "gamehub.db")
VERSION_FILE = os.path.join(RELEASES_DIR, "version.json")
BINARY_PATH = os.path.join(RELEASES_DIR, "LinuxGameHub.AppImage")

DEFAULT_ROOMS = ["POKOJ1", "POKOJ2"]
rooms: Dict[str, Dict[int, WebSocket]] = {r: {} for r in DEFAULT_ROOMS}


def verify_admin(credentials: HTTPBasicCredentials = Depends(security)):
    correct_user = secrets.compare_digest(credentials.username, ADMIN_USER)
    correct_pw = secrets.compare_digest(credentials.password, ADMIN_PASSWORD)
    if not (correct_user and correct_pw):
        raise HTTPException(
            status_code=status.HTTP_401_UNAUTHORIZED,
            detail="Błędny login lub hasło Administratora!",
            headers={"WWW-Authenticate": "Basic"},
        )
    return credentials.username


def get_version_info() -> dict:
    if os.path.exists(VERSION_FILE):
        try:
            with open(VERSION_FILE, "r", encoding="utf-8") as f:
                return json.load(f)
        except Exception:
            pass
    return {"version": "0.1.0", "changelog": "Pierwsze wydanie Fiksiarski Game Hub."}


def save_version_info(version: str, changelog: str):
    with open(VERSION_FILE, "w", encoding="utf-8") as f:
        json.dump({"version": version.strip(), "changelog": changelog.strip()}, f)


@contextmanager
def get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    try:
        yield conn
    finally:
        conn.close()


def init_db():
    with get_db() as conn:
        conn.execute("""
            CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                chips INTEGER DEFAULT 1000,
                chess_wins INTEGER DEFAULT 0,
                solitaire_wins INTEGER DEFAULT 0,
                blackjack_wins INTEGER DEFAULT 0,
                ludo_wins INTEGER DEFAULT 0,
                war_wins INTEGER DEFAULT 0,
                battleship_wins INTEGER DEFAULT 0,
                banned INTEGER DEFAULT 0
            )
        """)
        try:
            conn.execute("SELECT token FROM users LIMIT 1")
        except sqlite3.OperationalError:
            conn.execute("ALTER TABLE users ADD COLUMN token TEXT DEFAULT ''")
        conn.commit()


init_db()


def hash_pw(password: str) -> str:
    if HAS_BCRYPT:
        return bcrypt.hashpw(password.encode("utf-8"), bcrypt.gensalt()).decode("utf-8")
    salt = secrets.token_hex(16)
    h = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt.encode("utf-8"), 100_000).hex()
    return f"pbkdf2$100000${salt}${h}"


def check_pw(password: str, stored_hash: str) -> bool:
    if stored_hash.startswith("$2") and HAS_BCRYPT:
        return bcrypt.checkpw(password.encode("utf-8"), stored_hash.encode("utf-8"))
    if stored_hash.startswith("pbkdf2$"):
        parts = stored_hash.split("$")
        if len(parts) == 4:
            _, iters, salt, h = parts
            computed = hashlib.pbkdf2_hmac("sha256", password.encode("utf-8"), salt.encode("utf-8"), int(iters)).hex()
            return secrets.compare_digest(computed, h)
    # Legacy: plain SHA-256 (auto-migrated on next login)
    legacy = hashlib.sha256(password.encode("utf-8")).hexdigest()
    return secrets.compare_digest(legacy, stored_hash)


def user_to_profile(user_row, token: str = "") -> dict:
    return {
        "username": user_row["username"],
        "chips": user_row["chips"],
        "chess_wins": user_row["chess_wins"],
        "solitaire_wins": user_row["solitaire_wins"],
        "blackjack_wins": user_row["blackjack_wins"],
        "ludo_wins": user_row["ludo_wins"],
        "war_wins": user_row["war_wins"],
        "battleship_wins": user_row["battleship_wins"],
        "token": token,
    }


class AuthRequest(BaseModel):
    username: str
    password: str


class TokenAuthRequest(BaseModel):
    username: str
    token: str


class StatUpdateRequest(BaseModel):
    username: str
    token: str
    game: str
    win_delta: int = 0
    chips: int | None = None


@app.get("/favicon.ico", include_in_schema=False)
def favicon():
    return Response(content=b"", media_type="image/x-icon")


@app.post("/api/register")
def register(req: AuthRequest):
    username = req.username.strip()
    if len(username) < 3 or len(req.password) < 3:
        raise HTTPException(status_code=400, detail="Login i hasło muszą mieć min. 3 znaki.")
    if len(username) > 32 or len(req.password) > 128:
        raise HTTPException(status_code=400, detail="Login max 32 znaki, hasło max 128 znaków.")
    token = secrets.token_hex(32)
    with get_db() as conn:
        try:
            conn.execute(
                "INSERT INTO users (username, password_hash, chips, token) VALUES (?, ?, 1000, ?)",
                (username, hash_pw(req.password), token),
            )
            conn.commit()
        except sqlite3.IntegrityError:
            raise HTTPException(status_code=400, detail="Użytkownik o takim loginie już istnieje!")

        user = conn.execute("SELECT * FROM users WHERE username = ?", (username,)).fetchone()
    return user_to_profile(user, token)


@app.post("/api/login")
def login(req: AuthRequest):
    username = req.username.strip()
    if len(username) > 32 or len(req.password) > 128:
        raise HTTPException(status_code=400, detail="Login max 32 znaki, hasło max 128 znaków.")
    with get_db() as conn:
        user = conn.execute(
            "SELECT * FROM users WHERE username = ?",
            (username,),
        ).fetchone()

        if not user or not check_pw(req.password, user["password_hash"]):
            raise HTTPException(status_code=401, detail="Błędny login lub hasło!")
        if user["banned"] == 1:
            raise HTTPException(status_code=403, detail="To konto zostało zablokowane przez Admina!")

        # If legacy hash matched, upgrade to secure salted hash (bcrypt or PBKDF2)
        if not (user["password_hash"].startswith("$2") or (not HAS_BCRYPT and user["password_hash"].startswith("pbkdf2$"))):
            conn.execute("UPDATE users SET password_hash = ? WHERE id = ?", (hash_pw(req.password), user["id"]))

        # Generate new token
        token = secrets.token_hex(32)
        conn.execute("UPDATE users SET token = ? WHERE id = ?", (token, user["id"]))
        conn.commit()
    return user_to_profile(user, token)


@app.post("/api/token_login")
def token_login(req: TokenAuthRequest):
    with get_db() as conn:
        user = conn.execute("SELECT * FROM users WHERE username = ? AND token = ?",
                           (req.username.strip(), req.token)).fetchone()
        if not user or not req.token:
            raise HTTPException(status_code=401, detail="Token wygasł lub jest nieprawidłowy. Zaloguj się ponownie.")
        if user["banned"] == 1:
            raise HTTPException(status_code=403, detail="To konto zostało zablokowane przez Admina!")
        new_token = secrets.token_hex(32)
        conn.execute("UPDATE users SET token = ? WHERE id = ?", (new_token, user["id"]))
        conn.commit()
    return user_to_profile(user, new_token)


@app.post("/api/stats/update")
def update_stats(req: StatUpdateRequest):
    with get_db() as conn:
        user = conn.execute(
            "SELECT * FROM users WHERE username = ? AND token = ?",
            (req.username.strip(), req.token),
        ).fetchone()
        if not user or not req.token or user["banned"] == 1:
            raise HTTPException(status_code=401, detail="Brak autoryzacji")

        valid_games = {
            "chess": "chess_wins",
            "solitaire": "solitaire_wins",
            "blackjack": "blackjack_wins",
            "ludo": "ludo_wins",
            "war": "war_wins",
            "battleship": "battleship_wins",
        }

        if req.game in valid_games and 0 < req.win_delta <= 1:
            col = valid_games[req.game]
            conn.execute(f"UPDATE users SET {col} = {col} + ? WHERE id = ?", (req.win_delta, user["id"]))

        if req.chips is not None and req.game == "blackjack":
            current_chips = user["chips"]
            if abs(req.chips - current_chips) <= 750:
                conn.execute("UPDATE users SET chips = ? WHERE id = ?", (req.chips, user["id"]))

        conn.commit()
        updated = conn.execute("SELECT * FROM users WHERE id = ?", (user["id"],)).fetchone()
    return user_to_profile(updated, req.token)


@app.get("/api/leaderboard")
def leaderboard():
    with get_db() as conn:
        rows = conn.execute("""
            SELECT username, chips, chess_wins, solitaire_wins, blackjack_wins,
                   ludo_wins, war_wins, battleship_wins,
                   (chess_wins + solitaire_wins + blackjack_wins + ludo_wins + war_wins + battleship_wins) AS total_wins
            FROM users
            WHERE banned = 0
            ORDER BY total_wins DESC, chips DESC
            LIMIT 50
        """).fetchall()
    return [dict(r) for r in rows]


@app.get("/api/rooms")
def list_rooms():
    result = []
    for r_id in DEFAULT_ROOMS:
        active_map = rooms.get(r_id, {})
        result.append({
            "room_id": r_id,
            "players": len(active_map),
            "max_players": 4,
        })
    return result


@app.get("/api/version")
def check_version():
    info = get_version_info()
    has_binary = os.path.exists(BINARY_PATH)
    sha256 = ""
    if has_binary:
        h = hashlib.sha256()
        with open(BINARY_PATH, "rb") as f:
            for chunk in iter(lambda: f.read(8192), b""):
                h.update(chunk)
        sha256 = h.hexdigest()
    return {
        "version": info["version"],
        "changelog": info["changelog"],
        "has_binary": has_binary,
        "sha256": sha256,
    }


@app.get("/api/download_update")
def download_update():
    if not os.path.exists(BINARY_PATH):
        raise HTTPException(status_code=404, detail="Brak wgranego pliku aktualizacji na serwerze!")
    return FileResponse(
        BINARY_PATH,
        media_type="application/octet-stream",
        filename="LinuxGameHub.AppImage",
    )


@app.websocket("/ws/{room_id}")
async def websocket_endpoint(websocket: WebSocket, room_id: str):
    if room_id not in rooms:
        rooms[room_id] = {}

    free_slot = None
    for slot in range(4):
        if slot not in rooms[room_id]:
            free_slot = slot
            break

    if free_slot is None:
        await websocket.close(code=1008)
        return

    await websocket.accept()
    rooms[room_id][free_slot] = websocket
    total_players = len(rooms[room_id])

    await websocket.send_text(json.dumps({
        "type": "Assigned",
        "player_idx": free_slot,
        "players_count": total_players
    }))

    for slot_id, client in list(rooms[room_id].items()):
        if slot_id != free_slot:
            await client.send_text(json.dumps({
                "type": "PeerJoined",
                "players_count": total_players
            }))

    try:
        while True:
            data = await websocket.receive_text()
            try:
                parsed = json.loads(data)
                if not isinstance(parsed, dict) or "type" not in parsed:
                    continue
                allowed_types = {"ChessMove", "LudoRoll", "LudoMove", "WarStep",
                               "BattleshipFleet", "BattleshipShot",
                               "ResetRequest", "ResetApproved", "ResetDeclined"}
                if parsed["type"] not in allowed_types:
                    continue
            except (json.JSONDecodeError, KeyError):
                continue
            for slot_id, client in list(rooms[room_id].items()):
                if slot_id != free_slot:
                    await client.send_text(data)
    except WebSocketDisconnect:
        if room_id in rooms and free_slot in rooms[room_id]:
            del rooms[room_id][free_slot]
            rem_count = len(rooms[room_id])
            for client in list(rooms[room_id].values()):
                await client.send_text(json.dumps({
                    "type": "PeerJoined",
                    "players_count": rem_count
                }))


# --- STRONA GŁÓWNA ---

@app.get("/", response_class=HTMLResponse)
def home_page():
    ver_info = get_version_info()
    has_binary = os.path.exists(BINARY_PATH)
    version_str = ver_info.get("version", "0.1.0")
    github_url = "https://github.com/Kubson890/Fiksiarski-Game-Hub"
    download_url = "/api/download_update" if has_binary else f"{github_url}/releases"
    download_label = f"⬇ Pobierz v{version_str} (AppImage)" if has_binary else "⬇ Pobierz z GitHub Releases"

    return f"""
    <!DOCTYPE html>
    <html lang="pl">
    <head>
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>Fiksiarski Game Hub — Klasyczne Gry na Linuksa</title>
        <style>
            :root {{
                --bg: #11111b;
                --surface: #181825;
                --card-bg: #1e1e2e;
                --card-hover: #26263b;
                --border: #313244;
                --text: #cdd6f4;
                --subtext: #a6adc8;
                --accent: #89b4fa;
                --green: #a6e3a1;
                --gold: #f9e2af;
                --red: #f38ba8;
            }}
            * {{ box-sizing: border-box; margin: 0; padding: 0; }}
            body {{
                font-family: system-ui, -apple-system, "Segoe UI", Roboto, sans-serif;
                background-color: var(--bg);
                color: var(--text);
                line-height: 1.6;
                padding-bottom: 60px;
            }}
            a {{ color: inherit; text-decoration: none; }}

            /* NAVBAR */
            nav {{
                display: flex;
                justify-content: space-between;
                align-items: center;
                max-width: 1140px;
                margin: 0 auto;
                padding: 24px 20px;
                border-bottom: 1px solid var(--border);
            }}
            .logo {{
                font-size: 1.4rem;
                font-weight: 800;
                color: var(--accent);
                display: flex;
                align-items: center;
                gap: 10px;
            }}
            .nav-links {{
                display: flex;
                gap: 20px;
                align-items: center;
            }}
            .nav-link {{
                color: var(--subtext);
                font-weight: 500;
                transition: color 0.2s;
            }}
            .nav-link:hover {{ color: var(--accent); }}
            .admin-btn {{
                background: #313244;
                color: var(--text);
                padding: 7px 14px;
                border-radius: 8px;
                font-size: 0.9rem;
                font-weight: 600;
                transition: background 0.2s;
            }}
            .admin-btn:hover {{ background: #45475a; }}

            /* HERO */
            .hero {{
                text-align: center;
                max-width: 860px;
                margin: 70px auto 50px auto;
                padding: 0 20px;
            }}
            .version-pill {{
                display: inline-block;
                background: rgba(137, 180, 250, 0.12);
                color: var(--accent);
                border: 1px solid rgba(137, 180, 250, 0.3);
                padding: 6px 14px;
                border-radius: 20px;
                font-size: 0.88rem;
                font-weight: 600;
                margin-bottom: 24px;
            }}
            h1 {{
                font-size: 3rem;
                font-weight: 900;
                letter-spacing: -0.5px;
                color: #ffffff;
                margin-bottom: 20px;
                line-height: 1.2;
            }}
            .hero-desc {{
                font-size: 1.15rem;
                color: var(--subtext);
                margin-bottom: 34px;
                max-width: 720px;
                margin-left: auto;
                margin-right: auto;
            }}
            .cta-group {{
                display: flex;
                gap: 14px;
                justify-content: center;
                flex-wrap: wrap;
            }}
            .btn {{
                display: inline-flex;
                align-items: center;
                gap: 10px;
                padding: 13px 26px;
                border-radius: 10px;
                font-weight: 700;
                font-size: 1.05rem;
                transition: transform 0.15s, box-shadow 0.15s, background 0.2s;
            }}
            .btn:hover {{
                transform: translateY(-2px);
                box-shadow: 0 8px 24px rgba(0,0,0,0.4);
            }}
            .btn-primary {{
                background: var(--green);
                color: #11111b;
            }}
            .btn-primary:hover {{ background: #b4f0b0; }}
            .btn-secondary {{
                background: var(--card-bg);
                color: var(--text);
                border: 1px solid var(--border);
            }}
            .btn-secondary:hover {{
                background: var(--card-hover);
                border-color: var(--accent);
            }}

            /* SECTION WRAPPER */
            .section {{
                max-width: 1140px;
                margin: 60px auto 0 auto;
                padding: 0 20px;
            }}
            .section-title {{
                text-align: center;
                margin-bottom: 36px;
            }}
            .section-title h2 {{
                font-size: 2rem;
                color: #fff;
                margin-bottom: 8px;
            }}
            .section-title p {{
                color: var(--subtext);
                font-size: 1.05rem;
            }}

            /* GAMES GRID */
            .games-grid {{
                display: grid;
                grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
                gap: 22px;
            }}
            .game-card {{
                background: var(--card-bg);
                border: 1px solid var(--border);
                border-radius: 14px;
                padding: 26px;
                transition: transform 0.2s, border-color 0.2s, background 0.2s;
                position: relative;
                overflow: hidden;
            }}
            .game-card:hover {{
                transform: translateY(-3px);
                border-color: var(--accent);
                background: var(--card-hover);
            }}
            .game-icon {{
                font-size: 2.8rem;
                margin-bottom: 12px;
                display: inline-block;
            }}
            .game-name {{
                font-size: 1.35rem;
                font-weight: 700;
                color: #fff;
                margin-bottom: 6px;
            }}
            .game-badge {{
                display: inline-block;
                font-size: 0.75rem;
                font-weight: 700;
                text-transform: uppercase;
                letter-spacing: 0.6px;
                padding: 3px 8px;
                border-radius: 6px;
                margin-bottom: 12px;
                background: rgba(249, 226, 175, 0.15);
                color: var(--gold);
            }}
            .game-desc {{
                color: var(--subtext);
                font-size: 0.95rem;
                line-height: 1.5;
            }}

            /* FEATURES GRID */
            .features-grid {{
                display: grid;
                grid-template-columns: repeat(auto-fit, minmax(250px, 1fr));
                gap: 20px;
            }}
            .feature-box {{
                background: var(--surface);
                border: 1px solid var(--border);
                border-radius: 12px;
                padding: 24px;
                text-align: center;
            }}
            .feature-icon {{
                font-size: 2.2rem;
                margin-bottom: 10px;
            }}
            .feature-title {{
                font-size: 1.15rem;
                font-weight: 700;
                color: #fff;
                margin-bottom: 8px;
            }}
            .feature-desc {{
                font-size: 0.9rem;
                color: var(--subtext);
            }}

            /* CODE BOX */
            .code-box {{
                background: #0c0c14;
                border: 1px solid var(--border);
                border-radius: 10px;
                padding: 16px 20px;
                font-family: monospace;
                font-size: 0.95rem;
                color: var(--green);
                overflow-x: auto;
                margin: 20px auto;
                max-width: 600px;
                text-align: left;
            }}

            /* FOOTER */
            footer {{
                max-width: 1140px;
                margin: 80px auto 0 auto;
                padding: 30px 20px 0 20px;
                border-top: 1px solid var(--border);
                display: flex;
                justify-content: space-between;
                align-items: center;
                flex-wrap: wrap;
                gap: 16px;
                color: var(--subtext);
                font-size: 0.9rem;
            }}
            footer a:hover {{ color: var(--accent); }}
        </style>
    </head>
    <body>
        <nav>
            <div class="logo">
                <span>🎮</span>
                <span>Fiksiarski Game Hub</span>
            </div>
            <div class="nav-links">
                <a href="#gry" class="nav-link">Gry</a>
                <a href="#funkcje" class="nav-link">Funkcje</a>
                <a href="{github_url}" target="_blank" rel="noopener noreferrer" class="nav-link">GitHub 🐙</a>
                <a href="/admin" class="admin-btn">🔐 Panel Admina</a>
            </div>
        </nav>

        <header class="hero">
            <span class="version-pill">🎉 Wersja v{version_str} • Gotowy dla Linuksa</span>
            <h1>Klasyczne Gry na Linuksa w Nowoczesnym Wydaniu</h1>
            <p class="hero-desc">
                Lekkie, ultraszybkie centrum gier planszowych i karcianych napisane w języku <b>Rust</b>. 
                Graj solo z inteligentnymi botami lub twórz pokoje i rywalizuj ze znajomymi w czasie rzeczywistym!
            </p>
            <div class="cta-group">
                <a href="{download_url}" class="btn btn-primary">{download_label}</a>
                <a href="{github_url}" target="_blank" rel="noopener noreferrer" class="btn btn-secondary">
                    <span>🐙 Zobacz na GitHubie</span>
                </a>
            </div>
        </header>

        <section id="gry" class="section">
            <div class="section-title">
                <h2>6 Dostępnych Gier</h2>
                <p>Od strategicznych potyczek po emocjonujące gry karciane</p>
            </div>
            <div class="games-grid">
                <div class="game-card">
                    <span class="game-icon">♟️</span>
                    <h3 class="game-name">Szachy</h3>
                    <span class="game-badge">AI Bot • Multiplayer</span>
                    <p class="game-desc">
                        Pojedynki na szachownicy z pełną obsługą zasad, legalnymi ruchami, 
                        podglądem historii, botem AI z ewaluacją pozycji lub z żywym przeciwnikiem przez sieć.
                    </p>
                </div>

                <div class="game-card">
                    <span class="game-icon">🚢</span>
                    <h3 class="game-name">Statki (Battleship)</h3>
                    <span class="game-badge">Taktyczna • Radar</span>
                    <p class="game-desc">
                        Prawdziwa bitwa morska na siatkach 10x10. Rozstawiaj własną flotę ręcznie lub losowo 
                        i skanuj radar wroga, zatapiając kolejne okręty przeciwnika.
                    </p>
                </div>

                <div class="game-card">
                    <span class="game-icon">🎲</span>
                    <h3 class="game-name">Chińczyk (Ludo)</h3>
                    <span class="game-badge">2-4 Graczy • Kostka</span>
                    <p class="game-desc">
                        Ponadczasowa gra planszowa dla 2 do 4 graczy. Dynamiczne rzuty kostką, bezpieczne pola, 
                        zbijanie pionków rywali i wyścig do domków.
                    </p>
                </div>

                <div class="game-card">
                    <span class="game-icon">🃏</span>
                    <h3 class="game-name">Pasjans (Klondike)</h3>
                    <span class="game-badge">Solo • Drag & Drop</span>
                    <p class="game-desc">
                        Najpopularniejszy klasyczny Pasjans z wygodnym przeciąganiem kart, 
                        błyskawicznymi ruchami podwójnym kliknięciem i płynnymi animacjami lotu kart.
                    </p>
                </div>

                <div class="game-card">
                    <span class="game-icon">🎰</span>
                    <h3 class="game-name">Blackjack (21)</h3>
                    <span class="game-badge">Kasyno • Żetony</span>
                    <p class="game-desc">
                        Sprawdź swoje szczęście w blackjacku przeciwko krupierowi. Stawiaj żetony, 
                        dobieraj (Hit), pasuj (Stand), podwajaj stawkę (Double Down) i wspinaj się w rankingu.
                    </p>
                </div>

                <div class="game-card">
                    <span class="game-icon">⚔️</span>
                    <h3 class="game-name">Wojna (War)</h3>
                    <span class="game-badge">Karty • Szybka Akcja</span>
                    <p class="game-desc">
                        Emocjonujący pojedynek o całą talię kart. Rozstrzygaj wojny, przejmuj stosy kart 
                        i korzystaj z trybu szybkiej automatycznej rozgrywki.
                    </p>
                </div>
            </div>
        </section>

        <section id="funkcje" class="section">
            <div class="section-title">
                <h2>Dlaczego Fiksiarski Game Hub?</h2>
                <p>Zbudowany z myślą o prostocie, prędkości i dobrej zabawie</p>
            </div>
            <div class="features-grid">
                <div class="feature-box">
                    <div class="feature-icon">⚡</div>
                    <h3 class="feature-title">Czysty Rust & egui</h3>
                    <p class="feature-desc">Brak ociężałego Electrona. Aplikacja zużywa minimalną ilość pamięci RAM i uruchamia się natychmiastowo.</p>
                </div>
                <div class="feature-box">
                    <div class="feature-icon">🌐</div>
                    <h3 class="feature-title">Pokoje Multiplayer</h3>
                    <p class="feature-desc">Wspólne pokoje oparte na WebSocket pozwalają na płynną rozgrywkę wieloosobową bez zbędnej konfiguracji.</p>
                </div>
                <div class="feature-box">
                    <div class="feature-icon">🏆</div>
                    <h3 class="feature-title">Rankingi Graczy</h3>
                    <p class="feature-desc">Twoje wygrane w poszczególnych grach oraz zdobyte żetony są synchronizowane z globalną tabelą wyników.</p>
                </div>
                <div class="feature-box">
                    <div class="feature-icon">📦</div>
                    <h3 class="feature-title">Format AppImage</h3>
                    <p class="feature-desc">Jeden plik uruchamialny, który działa bez instalacji na Ubuntu, Debianie, Archu, Fedorze i innych dystrybucjach.</p>
                </div>
            </div>
        </section>

        <section class="section" style="text-align: center;">
            <div class="section-title">
                <h2>Jak uruchomić?</h2>
                <p>Tylko dwa polecenia w terminalu po pobraniu pliku AppImage:</p>
            </div>
            <div class="code-box">
                chmod +x FiksiarskiGameHub-x86_64.AppImage<br>
                ./FiksiarskiGameHub-x86_64.AppImage
            </div>
            <div style="margin-top: 26px;">
                <a href="{download_url}" class="btn btn-primary">{download_label}</a>
            </div>
        </section>

        <footer>
            <div>
                🎮 <b>Fiksiarski Game Hub</b> &copy; 2026. Projekt Open Source.
            </div>
            <div style="display: flex; gap: 20px;">
                <a href="{github_url}" target="_blank" rel="noopener noreferrer">GitHub</a>
                <a href="{github_url}/issues" target="_blank" rel="noopener noreferrer">Zgłoś błąd</a>
                <a href="/admin">Panel Administratora</a>
            </div>
        </footer>
    </body>
    </html>
    """


# --- PANEL WEBOWY ADMINA CHRONIONY HASŁEM ---

@app.get("/admin", response_class=HTMLResponse)
def admin_panel(admin: str = Depends(verify_admin)):
    with get_db() as conn:
        users = conn.execute("""
            SELECT *, (chess_wins + solitaire_wins + blackjack_wins + ludo_wins + war_wins + battleship_wins) AS total_wins
            FROM users ORDER BY id ASC
        """).fetchall()

    ver_info = get_version_info()
    bin_status = "✅ Wgrany na serwerze" if os.path.exists(BINARY_PATH) else "⚠ Brak pliku (wgraj poniżej)"
    p1_cnt = len(rooms.get("POKOJ1", {}))
    p2_cnt = len(rooms.get("POKOJ2", {}))

    rows_html = ""
    for u in users:
        status_badge = "<span style='color:#ff5555;'>ZBANOWANY</span>" if u["banned"] else "<span style='color:#50fa7b;'>AKTYWNY</span>"
        rows_html += f"""
        <tr>
            <td>{u['id']}</td>
            <td><b>{html.escape(u['username'])}</b></td>
            <td>{status_badge}</td>
            <td style='color:#ffd700;font-weight:bold;'>{u['chips']} 🪙</td>
            <td>{u['chess_wins']}</td>
            <td>{u['battleship_wins']}</td>
            <td>{u['ludo_wins']}</td>
            <td>{u['solitaire_wins']}</td>
            <td>{u['blackjack_wins']}</td>
            <td>{u['war_wins']}</td>
            <td><b>{u['total_wins']}</b></td>
            <td>
                <form method="post" action="/admin/update_user" style="display:inline-flex; gap:6px;">
                    <input type="hidden" name="user_id" value="{u['id']}">
                    <input type="number" name="chips" value="{u['chips']}" style="width:80px; background:#1e1e2e; color:white; border:1px solid #444; padding:4px; border-radius:4px;">
                    <button type="submit" style="background:#6272a4; color:white; border:none; padding:4px 8px; border-radius:4px; cursor:pointer;">Zapisz żetony</button>
                </form>
                <form method="post" action="/admin/toggle_ban" style="display:inline;">
                    <input type="hidden" name="user_id" value="{u['id']}">
                    <button type="submit" style="background:#ffb86c; color:#111; border:none; padding:4px 8px; border-radius:4px; cursor:pointer;">Ban/Unban</button>
                </form>
                <form method="post" action="/admin/reset_stats" style="display:inline;">
                    <input type="hidden" name="user_id" value="{u['id']}">
                    <button type="submit" style="background:#ff5555; color:white; border:none; padding:4px 8px; border-radius:4px; cursor:pointer;">Resetuj wygrane</button>
                </form>
            </td>
        </tr>
        """

    return f"""
    <!DOCTYPE html>
    <html>
    <head>
        <meta charset="utf-8">
        <title>Fiksiarski Game Hub - Panel Admina</title>
        <style>
            body {{ font-family: system-ui, sans-serif; background: #181825; color: #cdd6f4; margin: 0; padding: 30px; }}
            h1, h2 {{ color: #89b4fa; margin-bottom: 8px; }}
            table {{ width: 100%; border-collapse: collapse; background: #1e1e2e; border-radius: 10px; overflow: hidden; margin-top: 20px; }}
            th, td {{ padding: 12px 14px; text-align: left; border-bottom: 1px solid #313244; }}
            th {{ background: #11111b; color: #a6adc8; }}
            tr:hover {{ background: #262639; }}
            .card {{ background: #1e1e2e; padding: 18px; border-radius: 10px; margin-bottom: 20px; border: 1px solid #313244; }}
            input[type="text"], input[type="file"] {{ background:#11111b; color:white; border:1px solid #45475a; padding:6px 10px; border-radius:6px; }}
        </style>
    </head>
    <body>
        <h1>🎮 Fiksiarski Game Hub — Panel Administratora (Zalogowano: {admin})</h1>
        <div class="card">
            <b>Port serwera:</b> 🟢 {SERVER_PORT} &nbsp;|&nbsp;
            <b>POKOJ1:</b> {p1_cnt}/4 graczy &nbsp;|&nbsp;
            <b>POKOJ2:</b> {p2_cnt}/4 graczy &nbsp;|&nbsp;
            <b>Zarejestrowani gracze:</b> {len(users)}
        </div>

        <div class="card">
            <h2>🚀 System Auto-Aktualizacji (Wypuść nową wersję gry)</h2>
            <p>Aktualna wersja na serwerze: <b style="color:#a6e3a1;">v{ver_info['version']}</b> &nbsp;|&nbsp; Plik AppImage: <b>{bin_status}</b></p>
            <form method="post" action="/admin/publish_release" enctype="multipart/form-data" style="display:flex; gap:12px; align-items:center; flex-wrap:wrap;">
                <label>Nowa wersja: <input type="text" name="version" value="{ver_info['version']}" required style="width:90px;"></label>
                <label>Opis zmian: <input type="text" name="changelog" value="{ver_info['changelog']}" style="width:280px;"></label>
                <label>Plik AppImage / Binarka: <input type="file" name="binary_file"></label>
                <button type="submit" style="background:#a6e3a1; color:#11111b; font-weight:bold; border:none; padding:8px 16px; border-radius:6px; cursor:pointer;">⬆ Opublikuj aktualizację</button>
            </form>
        </div>

        <table>
            <thead>
                <tr>
                    <th>ID</th>
                    <th>Gracz</th>
                    <th>Status</th>
                    <th>Żetony (BJ)</th>
                    <th>♟ Szachy</th>
                    <th>🚢 Statki</th>
                    <th>🎲 Chińczyk</th>
                    <th>🃏 Pasjans</th>
                    <th>🎰 Blackjack</th>
                    <th>⚔ Wojna</th>
                    <th>🏆 Suma</th>
                    <th>Akcje Admina</th>
                </tr>
            </thead>
            <tbody>
                {rows_html}
            </tbody>
        </table>
    </body>
    </html>
    """


@app.post("/admin/publish_release")
async def admin_publish_release(
    version: str = Form(...),
    changelog: str = Form(""),
    binary_file: UploadFile | None = File(None),
    _admin: str = Depends(verify_admin),
):
    save_version_info(version, changelog)
    if binary_file and binary_file.filename:
        content = await binary_file.read()
        if len(content) > 0:
            with open(BINARY_PATH, "wb") as f:
                f.write(content)
    return RedirectResponse(url="/admin", status_code=303)


@app.post("/admin/update_user")
def admin_update_user(
    user_id: int = Form(...),
    chips: int = Form(...),
    _admin: str = Depends(verify_admin),
):
    with get_db() as conn:
        conn.execute("UPDATE users SET chips = ? WHERE id = ?", (chips, user_id))
        conn.commit()
    return RedirectResponse(url="/admin", status_code=303)


@app.post("/admin/toggle_ban")
def admin_toggle_ban(
    user_id: int = Form(...),
    _admin: str = Depends(verify_admin),
):
    with get_db() as conn:
        conn.execute("UPDATE users SET banned = CASE WHEN banned = 1 THEN 0 ELSE 1 END WHERE id = ?", (user_id,))
        conn.commit()
    return RedirectResponse(url="/admin", status_code=303)


@app.post("/admin/reset_stats")
def admin_reset_stats(
    user_id: int = Form(...),
    _admin: str = Depends(verify_admin),
):
    with get_db() as conn:
        conn.execute("""
            UPDATE users SET chess_wins=0, solitaire_wins=0, blackjack_wins=0,
            ludo_wins=0, war_wins=0, battleship_wins=0 WHERE id = ?
        """, (user_id,))
        conn.commit()
    return RedirectResponse(url="/admin", status_code=303)


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=SERVER_PORT)
