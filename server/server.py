import os
import sqlite3
import hashlib
import secrets
import json
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
ADMIN_PASSWORD = os.environ.get("ADMIN_PASSWORD", "admin123")

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


def get_db():
    conn = sqlite3.connect(DB_PATH)
    conn.row_factory = sqlite3.Row
    return conn


def init_db():
    conn = get_db()
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
    conn.commit()
    conn.close()


init_db()


def hash_pw(password: str) -> str:
    return hashlib.sha256(password.encode("utf-8")).hexdigest()


class AuthRequest(BaseModel):
    username: str
    password: str


class StatUpdateRequest(BaseModel):
    username: str
    password: str
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
    conn = get_db()
    try:
        conn.execute(
            "INSERT INTO users (username, password_hash, chips) VALUES (?, ?, 1000)",
            (username, hash_pw(req.password)),
        )
        conn.commit()
    except sqlite3.IntegrityError:
        conn.close()
        raise HTTPException(status_code=400, detail="Użytkownik o takim loginie już istnieje!")

    user = conn.execute("SELECT * FROM users WHERE username = ?", (username,)).fetchone()
    conn.close()
    return dict(user)


@app.post("/api/login")
def login(req: AuthRequest):
    conn = get_db()
    user = conn.execute(
        "SELECT * FROM users WHERE username = ? AND password_hash = ?",
        (req.username.strip(), hash_pw(req.password)),
    ).fetchone()
    conn.close()

    if not user:
        raise HTTPException(status_code=401, detail="Błędny login lub hasło!")
    if user["banned"] == 1:
        raise HTTPException(status_code=403, detail="To konto zostało zablokowane przez Admina!")
    return dict(user)


@app.post("/api/stats/update")
def update_stats(req: StatUpdateRequest):
    conn = get_db()
    user = conn.execute(
        "SELECT * FROM users WHERE username = ? AND password_hash = ?",
        (req.username.strip(), hash_pw(req.password)),
    ).fetchone()
    if not user or user["banned"] == 1:
        conn.close()
        raise HTTPException(status_code=401, detail="Brak autoryzacji")

    valid_games = {
        "chess": "chess_wins",
        "solitaire": "solitaire_wins",
        "blackjack": "blackjack_wins",
        "ludo": "ludo_wins",
        "war": "war_wins",
        "battleship": "battleship_wins",
    }

    if req.game in valid_games and req.win_delta > 0:
        col = valid_games[req.game]
        conn.execute(f"UPDATE users SET {col} = {col} + ? WHERE id = ?", (req.win_delta, user["id"]))

    if req.chips is not None:
        conn.execute("UPDATE users SET chips = ? WHERE id = ?", (req.chips, user["id"]))

    conn.commit()
    updated = conn.execute("SELECT * FROM users WHERE id = ?", (user["id"],)).fetchone()
    conn.close()
    return dict(updated)


@app.get("/api/leaderboard")
def leaderboard():
    conn = get_db()
    rows = conn.execute("""
        SELECT username, chips, chess_wins, solitaire_wins, blackjack_wins,
               ludo_wins, war_wins, battleship_wins,
               (chess_wins + solitaire_wins + blackjack_wins + ludo_wins + war_wins + battleship_wins) AS total_wins
        FROM users
        WHERE banned = 0
        ORDER BY total_wins DESC, chips DESC
        LIMIT 50
    """).fetchall()
    conn.close()
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
    return {
        "version": info["version"],
        "changelog": info["changelog"],
        "has_binary": has_binary,
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


# --- PANEL WEBOWY ADMINA CHRONIONY HASŁEM ---

@app.get("/admin", response_class=HTMLResponse)
def admin_panel(admin: str = Depends(verify_admin)):
    conn = get_db()
    users = conn.execute("""
        SELECT *, (chess_wins + solitaire_wins + blackjack_wins + ludo_wins + war_wins + battleship_wins) AS total_wins
        FROM users ORDER BY id ASC
    """).fetchall()
    conn.close()

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
            <td><b>{u['username']}</b></td>
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
    conn = get_db()
    conn.execute("UPDATE users SET chips = ? WHERE id = ?", (chips, user_id))
    conn.commit()
    conn.close()
    return RedirectResponse(url="/admin", status_code=303)


@app.post("/admin/toggle_ban")
def admin_toggle_ban(
    user_id: int = Form(...),
    _admin: str = Depends(verify_admin),
):
    conn = get_db()
    conn.execute("UPDATE users SET banned = CASE WHEN banned = 1 THEN 0 ELSE 1 END WHERE id = ?", (user_id,))
    conn.commit()
    conn.close()
    return RedirectResponse(url="/admin", status_code=303)


@app.post("/admin/reset_stats")
def admin_reset_stats(
    user_id: int = Form(...),
    _admin: str = Depends(verify_admin),
):
    conn = get_db()
    conn.execute("""
        UPDATE users SET chess_wins=0, solitaire_wins=0, blackjack_wins=0,
        ludo_wins=0, war_wins=0, battleship_wins=0 WHERE id = ?
    """, (user_id,))
    conn.commit()
    conn.close()
    return RedirectResponse(url="/admin", status_code=303)


if __name__ == "__main__":
    uvicorn.run(app, host="0.0.0.0", port=SERVER_PORT)