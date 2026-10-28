import os
import sys
import sqlite3
import hashlib
import secrets
import json
import html
import time
from contextlib import contextmanager
from collections import defaultdict

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
from fastapi import FastAPI, HTTPException, Form, UploadFile, File, WebSocket, WebSocketDisconnect, Depends, status, Request
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

# Osobne ścieżki dla Linuksa i Windowsa
LINUX_BINARY_PATH = os.path.join(RELEASES_DIR, "FiksiarskiGameHub-x86_64.AppImage")
WINDOWS_BINARY_PATH = os.path.join(RELEASES_DIR, "fiksiarski_game_hub.exe")

DEFAULT_ROOMS = ["POKOJ1", "POKOJ2"]
rooms: Dict[str, Dict[int, WebSocket]] = {r: {} for r in DEFAULT_ROOMS}
room_order: Dict[str, list[int]] = {r: [] for r in DEFAULT_ROOMS}
room_focus: Dict[str, list[str]] = {r: [] for r in DEFAULT_ROOMS}
room_resets: Dict[str, Dict[int, dict | None]] = {
    r: {0: None, 1: None, 2: None} for r in DEFAULT_ROOMS
}

TWO_PLAYER_GAMES = {"chess", "battleship"}
KNOWN_GAMES = {"chess", "battleship", "ludo"}

_login_attempts: dict = defaultdict(list)
_LOGIN_LIMIT = 10
_LOGIN_WINDOW = 60


def _check_rate_limit(ip: str):
    now = time.time()
    attempts = _login_attempts[ip]
    _login_attempts[ip] = [t for t in attempts if now - t < _LOGIN_WINDOW]
    if len(_login_attempts[ip]) >= _LOGIN_LIMIT:
        raise HTTPException(
            status_code=429,
            detail="Zbyt wiele prób logowania. Spróbuj ponownie za chwilę."
        )
    _login_attempts[ip].append(now)


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


def _file_sha256(path: str) -> str:
    if not os.path.exists(path):
        return ""
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(8192), b""):
            h.update(chunk)
    return h.hexdigest()


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
def register(req: AuthRequest, request: Request):
    ip = request.client.host if request.client else "unknown"
    _check_rate_limit(ip)
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
def login(req: AuthRequest, request: Request):
    ip = request.client.host if request.client else "unknown"
    _check_rate_limit(ip)
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

        if not (user["password_hash"].startswith("$2") or (not HAS_BCRYPT and user["password_hash"].startswith("pbkdf2$"))):
            conn.execute("UPDATE users SET password_hash = ? WHERE id = ?", (hash_pw(req.password), user["id"]))

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
def check_version(platform: str = "linux"):
    info = get_version_info()
    # Rozpoznajemy platformę klienta (domyślnie linux, opcjonalnie windows)
    is_win = platform.lower() == "windows"
    bin_path = WINDOWS_BINARY_PATH if is_win else LINUX_BINARY_PATH
    has_binary = os.path.exists(bin_path)
    sha256 = _file_sha256(bin_path)

    return {
        "version": info["version"],
        "changelog": info["changelog"],
        "has_binary": has_binary,
        "sha256": sha256,
        "platform": "windows" if is_win else "linux",
    }


@app.get("/api/download_update")
def download_update(platform: str = "linux"):
    is_win = platform.lower() == "windows"
    bin_path = WINDOWS_BINARY_PATH if is_win else LINUX_BINARY_PATH
    filename = "fiksiarski_game_hub.exe" if is_win else "FiksiarskiGameHub-x86_64.AppImage"

    if not os.path.exists(bin_path):
        raise HTTPException(
            status_code=404, 
            detail=f"Brak pliku aktualizacji dla platformy: {'Windows' if is_win else 'Linux'}!"
        )

    return FileResponse(
        bin_path,
        media_type="application/octet-stream",
        filename=filename,
    )


def _focus_list(room_id: str) -> list[str]:
    order = room_order.get(room_id, [])
    focus = list(room_focus.get(room_id, []))
    if len(focus) < len(order):
        focus.extend(["hub"] * (len(order) - len(focus)))
    return focus[:len(order)]


def _slot_focus(room_id: str, slot: int) -> str | None:
    order = room_order.get(room_id, [])
    try:
        idx = order.index(slot)
    except ValueError:
        return None
    focus = _focus_list(room_id)
    return focus[idx] if idx < len(focus) else "hub"


def _pair_active(room_id: str, pair: int, game: str) -> bool:
    if pair not in (0, 1):
        return False
    return (
        _slot_focus(room_id, pair * 2) == game
        and _slot_focus(room_id, pair * 2 + 1) == game
    )


def _room_state(room_id: str) -> dict:
    order = list(room_order.get(room_id, []))
    return {
        "type": "RoomState",
        "order": order,
        "players_count": len(order),
        "focus": _focus_list(room_id),
    }


async def _broadcast_json(room_id: str, payload: dict, exclude_slot: int | None = None):
    data = json.dumps(payload)
    for slot_id, client in list(rooms.get(room_id, {}).items()):
        if exclude_slot is not None and slot_id == exclude_slot:
            continue
        try:
            await client.send_text(data)
        except Exception:
            pass


async def _broadcast_room_state(room_id: str, exclude_slot: int | None = None):
    await _broadcast_json(room_id, _room_state(room_id), exclude_slot)


async def _prune_room_resets(room_id: str):
    resets = room_resets.get(room_id, {})
    for key, pending in list(resets.items()):
        if pending is None:
            continue
        game = pending.get("game", "")
        valid = pending.get("from_slot") in room_order.get(room_id, [])
        if valid and game in TWO_PLAYER_GAMES:
            valid = _pair_active(room_id, key, game)
        if not valid:
            resets[key] = None
            await _broadcast_json(room_id, {
                "type": "ResetDeclined",
                "game": game,
                "pair": key,
                "from_seat": pending.get("from_seat", 0),
            })


@app.websocket("/ws/{room_id}")
async def websocket_endpoint(websocket: WebSocket, room_id: str):
    if room_id not in DEFAULT_ROOMS:
        await websocket.close(code=1008)
        return

    free_slot = None
    for slot in range(4):
        if slot not in rooms[room_id]:
            free_slot = slot
            break

    if free_slot is None:
        await websocket.close(code=1008)
        return

    rooms[room_id][free_slot] = websocket
    try:
        await websocket.accept()
    except Exception:
        rooms[room_id].pop(free_slot, None)
        return

    room_focus[room_id] = _focus_list(room_id)
    room_order[room_id].append(free_slot)
    room_focus[room_id].append("hub")
    seat = room_order[room_id].index(free_slot)
    total_players = len(room_order[room_id])

    await websocket.send_text(json.dumps({
        "type": "Assigned",
        "slot": free_slot,
        "seat": seat,
        "players_count": total_players,
        "order": list(room_order[room_id]),
        "focus": _focus_list(room_id),
    }))

    await _broadcast_room_state(room_id, exclude_slot=free_slot)

    try:
        while True:
            data = await websocket.receive_text()
            try:
                parsed = json.loads(data)
                if not isinstance(parsed, dict) or "type" not in parsed:
                    continue
                allowed_types = {"ChessMove", "LudoRoll", "LudoMove", "WarStep",
                               "BattleshipFleet", "BattleshipShot",
                               "ResetRequest", "ResetApproved", "ResetDeclined",
                               "StateRequest", "GameState", "FocusGame"}
                if parsed["type"] not in allowed_types:
                    continue
            except (json.JSONDecodeError, KeyError):
                continue

            if free_slot not in room_order.get(room_id, []):
                continue
            seat = room_order[room_id].index(free_slot)
            event_type = parsed["type"]

            if event_type == "FocusGame":
                game = str(parsed.get("game", ""))
                if game not in {"hub", "chess", "battleship", "ludo"}:
                    continue
                focus = _focus_list(room_id)
                if focus[seat] != game:
                    focus[seat] = game
                    room_focus[room_id] = focus
                    await _broadcast_room_state(room_id)
                    await _prune_room_resets(room_id)
                continue

            requested_pair_raw = parsed.get("pair")
            pair = free_slot // 2
            parsed["pair"] = pair
            parsed["from_seat"] = free_slot % 2

            event_game = {
                "ChessMove": "chess",
                "BattleshipFleet": "battleship",
                "BattleshipShot": "battleship",
            }.get(event_type)
            if event_game is not None and not _pair_active(room_id, pair, event_game):
                continue

            if event_type == "WarStep" and pair != 0:
                continue

            if event_type in {"LudoRoll", "LudoMove"}:
                parsed["pair"] = 2

            broadcast_to_all = False
            if event_type in {"ResetRequest", "ResetApproved", "ResetDeclined"}:
                game = str(parsed.get("game", ""))
                if game not in KNOWN_GAMES:
                    continue

                if game in TWO_PLAYER_GAMES:
                    reset_key = pair
                    if not _pair_active(room_id, reset_key, game):
                        continue
                else:
                    reset_key = 2

                pending = room_resets[room_id].get(reset_key)

                if event_type == "ResetRequest":
                    if pending is not None:
                        continue
                    room_resets[room_id][reset_key] = {
                        "from_seat": free_slot % 2,
                        "from_slot": free_slot,
                        "game": game,
                    }
                else:
                    if pending is None:
                        continue
                    game = pending["game"]
                    if free_slot == pending.get("from_slot"):
                        continue
                    claimed = str(parsed.get("game", ""))
                    if claimed and claimed != game:
                        continue
                    room_resets[room_id][reset_key] = None
                    broadcast_to_all = True

                parsed["game"] = game
                parsed["pair"] = reset_key
                if event_type == "ResetRequest":
                    parsed["from_player"] = free_slot % 2

            if event_type in {"StateRequest", "GameState"}:
                game = str(parsed.get("game", ""))
                if game not in KNOWN_GAMES:
                    continue
                if game in TWO_PLAYER_GAMES:
                    if event_type == "StateRequest":
                        try:
                            requested_pair = int(
                                requested_pair_raw if requested_pair_raw is not None else -1
                            )
                        except (TypeError, ValueError):
                            continue
                        if not _pair_active(room_id, requested_pair, game):
                            continue
                        parsed["pair"] = requested_pair
                    else:
                        if not _pair_active(room_id, pair, game):
                            continue
                        parsed["pair"] = pair
                else:
                    if event_type == "GameState" and seat >= 4:
                        continue
                    parsed["pair"] = 2

            data = json.dumps(parsed)

            for slot_id, client in list(rooms[room_id].items()):
                if slot_id == free_slot and not broadcast_to_all:
                    continue
                try:
                    await client.send_text(data)
                except Exception:
                    pass
    except WebSocketDisconnect:
        pass
    finally:
        if room_id in rooms and free_slot in rooms[room_id]:
            del rooms[room_id][free_slot]
        if room_id in room_order and free_slot in room_order[room_id]:
            room_focus[room_id] = _focus_list(room_id)
            focus_idx = room_order[room_id].index(free_slot)
            room_order[room_id].remove(free_slot)
            if focus_idx < len(room_focus[room_id]):
                room_focus[room_id].pop(focus_idx)

        await _broadcast_room_state(room_id)
        await _prune_room_resets(room_id)


# --- PANEL WEBOWY ADMINA ---

@app.get("/admin", response_class=HTMLResponse)
def admin_panel(admin: str = Depends(verify_admin)):
    with get_db() as conn:
        users = conn.execute("""
            SELECT *, (chess_wins + solitaire_wins + blackjack_wins + ludo_wins + war_wins + battleship_wins) AS total_wins
            FROM users ORDER BY id ASC
        """).fetchall()

    ver_info = get_version_info()
    linux_status = "✅ Wgrany (.AppImage)" if os.path.exists(LINUX_BINARY_PATH) else "⚠ Brak pliku"
    win_status = "✅ Wgrany (.exe)" if os.path.exists(WINDOWS_BINARY_PATH) else "⚠ Brak pliku"
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
            <h2>🚀 System Auto-Aktualizacji (Multiplatform)</h2>
            <p>Aktualna wersja na serwerze: <b style="color:#a6e3a1;">v{ver_info['version']}</b></p>
            <p>Linux (.AppImage): <b>{linux_status}</b> &nbsp;|&nbsp; Windows (.exe): <b>{win_status}</b></p>
            <form method="post" action="/admin/publish_release" enctype="multipart/form-data" style="display:flex; flex-direction:column; gap:12px; margin-top:12px;">
                <div style="display:flex; gap:16px; flex-wrap:wrap;">
                    <label>Wersja: <input type="text" name="version" value="{ver_info['version']}" required style="width:90px;"></label>
                    <label>Opis zmian: <input type="text" name="changelog" value="{ver_info['changelog']}" style="width:320px;"></label>
                </div>
                <div style="display:flex; gap:16px; flex-wrap:wrap;">
                    <label>🐧 Plik Linux (.AppImage): <input type="file" name="linux_file"></label>
                    <label>🪟 Plik Windows (.exe): <input type="file" name="windows_file"></label>
                </div>
                <div>
                    <button type="submit" style="background:#a6e3a1; color:#11111b; font-weight:bold; border:none; padding:8px 18px; border-radius:6px; cursor:pointer;">⬆ Opublikuj aktualizację</button>
                </div>
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
    linux_file: UploadFile | None = File(None),
    windows_file: UploadFile | None = File(None),
    _admin: str = Depends(verify_admin),
):
    save_version_info(version, changelog)
    if linux_file and linux_file.filename:
        content = await linux_file.read()
        if len(content) > 0:
            with open(LINUX_BINARY_PATH, "wb") as f:
                f.write(content)
    if windows_file and windows_file.filename:
        content = await windows_file.read()
        if len(content) > 0:
            with open(WINDOWS_BINARY_PATH, "wb") as f:
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