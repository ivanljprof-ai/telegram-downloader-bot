import os
import sys
import json
import time
import urllib.parse
import urllib.request
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler

# Datos compartidos globalmente tras recibir la configuración
LATEST_CONFIG = {
    "token": "",
    "user_id": "",
    "insta_user": "",
    "insta_pass": "",
    "cookies_txt": ""
}

HTML_FORM_TEMPLATE = """<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Configuración del Bot Multidescargas</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: 'Inter', sans-serif; }
        body {
            background: radial-gradient(circle at top left, #1a1c2e, #0d0e15);
            color: #f3f4f6;
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }
        .container {
            width: 100%;
            max-width: 540px;
            background: rgba(255, 255, 255, 0.03);
            backdrop-filter: blur(16px);
            -webkit-backdrop-filter: blur(16px);
            border: 1px solid rgba(255, 255, 255, 0.08);
            border-radius: 20px;
            padding: 40px;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.5), inset 0 1px 0 rgba(255, 255, 255, 0.1);
        }
        .header { text-align: center; margin-bottom: 30px; }
        .header .icon { font-size: 44px; margin-bottom: 10px; display: inline-block; }
        .header h1 {
            font-size: 24px; font-weight: 700;
            background: linear-gradient(135deg, #60a5fa, #a855f7);
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
            margin-bottom: 8px;
        }
        .header p { color: #9ca3af; font-size: 14px; line-height: 1.5; }
        .form-group { margin-bottom: 20px; }
        label { display: block; font-size: 13px; font-weight: 600; color: #d1d5db; margin-bottom: 8px; }
        label span.required { color: #f87171; }
        label span.optional { color: #6b7280; font-weight: 400; font-size: 12px; }
        input[type="text"], input[type="password"], textarea {
            width: 100%; background: rgba(0, 0, 0, 0.3);
            border: 1px solid rgba(255, 255, 255, 0.12);
            border-radius: 10px; padding: 12px 16px; color: #ffffff;
            font-size: 14px; outline: none; transition: all 0.2s ease;
        }
        input[type="text"]:focus, input[type="password"]:focus, textarea:focus {
            border-color: #60a5fa; box-shadow: 0 0 0 3px rgba(96, 165, 250, 0.2);
            background: rgba(0, 0, 0, 0.5);
        }
        textarea { resize: vertical; min-height: 90px; font-family: 'JetBrains Mono', monospace; font-size: 12px; }
        .btn-submit {
            width: 100%; background: linear-gradient(135deg, #3b82f6, #8b5cf6);
            border: none; border-radius: 12px; color: white; font-weight: 600;
            font-size: 15px; padding: 14px; cursor: pointer; transition: all 0.3s ease;
            box-shadow: 0 10px 25px -5px rgba(59, 130, 246, 0.4); margin-top: 10px;
        }
        .btn-submit:hover {
            transform: translateY(-2px);
            box-shadow: 0 15px 30px -5px rgba(59, 130, 246, 0.6);
            background: linear-gradient(135deg, #2563eb, #7c3aed);
        }
        .footer-note {
            margin-top: 25px; text-align: center; font-size: 12px; color: #6b7280;
            border-top: 1px solid rgba(255, 255, 255, 0.05); padding-top: 15px;
        }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <span class="icon">🚀</span>
            <h1>Configuración Inicial del Bot</h1>
            <p>Introduce tus datos para inicializar el Bot Multidescargas. Verás el progreso de arranque en pantalla en tiempo real.</p>
        </div>
        <form method="POST" action="/">
            <div class="form-group">
                <label for="telegram_token">🔑 Token del Bot de Telegram <span class="required">*</span></label>
                <input type="text" id="telegram_token" name="telegram_token" placeholder="123456789:ABCdefGhIJKlmNoPQRstUVwxyZ" required>
            </div>
            
            <div class="form-group">
                <label for="allowed_user_id">👤 ID de Usuario Autorizado (Telegram) <span class="required">*</span></label>
                <input type="text" id="allowed_user_id" name="allowed_user_id" placeholder="123456789" required>
            </div>
            
            <div class="form-group">
                <label for="instagram_user">📸 Usuario de Instagram <span class="optional">(Opcional)</span></label>
                <input type="text" id="instagram_user" name="instagram_user" placeholder="tu_usuario">
            </div>
            
            <div class="form-group">
                <label for="instagram_password">🔒 Contraseña de Instagram <span class="optional">(Opcional)</span></label>
                <input type="password" id="instagram_password" name="instagram_password" placeholder="••••••••">
            </div>

            <div class="form-group">
                <label for="download_path">📁 Ruta de Almacenamiento <span class="optional">(Predeterminado: ./downloads)</span></label>
                <input type="text" id="download_path" name="download_path" value="./downloads" placeholder="./downloads">
            </div>

            <div class="form-group">
                <label for="cookies">🍪 Contenido de cookies.txt (Instagram) <span class="optional">(Opcional)</span></label>
                <textarea id="cookies" name="cookies" placeholder="# Netscape HTTP Cookie File... Pega aquí el contenido de cookies.txt"></textarea>
            </div>
            
            <button type="submit" class="btn-submit">⚡ Guardar e Iniciar Sistema</button>
        </form>
        <div class="footer-note">
            ⚡ Configuración inicial automatizada de 1 solo clic.
        </div>
    </div>
</body>
</html>
"""

HTML_TERMINAL_TEMPLATE = """<!DOCTYPE html>
<html lang="es">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Iniciando Sistema...</title>
    <link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;600;700&family=JetBrains+Mono:wght@400;500&display=swap" rel="stylesheet">
    <style>
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: 'Inter', sans-serif; }
        body {
            background: #090a0f;
            color: #f3f4f6;
            min-height: 100vh;
            display: flex;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }
        .container {
            width: 100%;
            max-width: 650px;
            background: rgba(255, 255, 255, 0.03);
            border: 1px solid rgba(255, 255, 255, 0.08);
            border-radius: 20px;
            padding: 30px;
            box-shadow: 0 20px 50px rgba(0, 0, 0, 0.6);
        }
        .header {
            display: flex;
            align-items: center;
            justify-content: space-between;
            margin-bottom: 20px;
            padding-bottom: 15px;
            border-bottom: 1px solid rgba(255, 255, 255, 0.08);
        }
        .header-title {
            display: flex;
            align-items: center;
            gap: 12px;
        }
        .status-dot {
            width: 10px;
            height: 10px;
            border-radius: 50%;
            background: #f59e0b;
            box-shadow: 0 0 10px #f59e0b;
            animation: pulse 1.5s infinite;
        }
        .status-dot.active {
            background: #10b981;
            box-shadow: 0 0 12px #10b981;
        }
        @keyframes pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.4; } }
        h2 { font-size: 18px; font-weight: 700; color: #ffffff; }
        
        .terminal {
            background: #050508;
            border: 1px solid rgba(255, 255, 255, 0.1);
            border-radius: 12px;
            padding: 20px;
            height: 240px;
            overflow-y: auto;
            font-family: 'JetBrains Mono', monospace;
            font-size: 13px;
            color: #34d399;
            line-height: 1.6;
            margin-bottom: 25px;
            box-shadow: inset 0 2px 8px rgba(0, 0, 0, 0.8);
        }
        .terminal p { margin-bottom: 6px; word-break: break-word; }
        .terminal .error { color: #f87171; }
        .terminal .info { color: #60a5fa; }
        .terminal .success { color: #34d399; font-weight: 600; }

        .actions {
            display: none;
            flex-direction: column;
            gap: 12px;
            animation: fadeIn 0.5s ease forwards;
        }
        @keyframes fadeIn { from { opacity: 0; transform: translateY(10px); } to { opacity: 1; transform: translateY(0); } }

        .btn-telegram {
            display: flex; align-items: center; justify-content: center; gap: 10px;
            width: 100%; background: linear-gradient(135deg, #2563eb, #0284c7);
            color: white; border: none; border-radius: 12px; padding: 14px;
            font-weight: 600; font-size: 15px; text-decoration: none;
            box-shadow: 0 10px 25px -5px rgba(37, 99, 235, 0.4);
            transition: all 0.2s ease; cursor: pointer;
        }
        .btn-telegram:hover {
            transform: translateY(-2px);
            box-shadow: 0 15px 30px -5px rgba(37, 99, 235, 0.6);
        }
        .btn-close {
            background: rgba(255, 255, 255, 0.05);
            border: 1px solid rgba(255, 255, 255, 0.1);
            color: #9ca3af; border-radius: 12px; padding: 12px;
            font-size: 14px; font-weight: 500; cursor: pointer; text-align: center;
            transition: all 0.2s ease;
        }
        .btn-close:hover { background: rgba(255, 255, 255, 0.1); color: #ffffff; }
    </style>
</head>
<body>
    <div class="container">
        <div class="header">
            <div class="header-title">
                <div class="status-dot" id="statusDot"></div>
                <h2 id="statusTitle">Inicializando Bot...</h2>
            </div>
            <span style="font-size: 12px; color: #6b7280;" id="statusSubtitle">En proceso</span>
        </div>

        <div class="terminal" id="terminal">
            <p class="info">[00:00] ⚙️ Iniciando conexión con el servidor...</p>
        </div>

        <div class="actions" id="actions">
            <a href="#" id="telegramBtn" target="_blank" class="btn-telegram">
                📱 Abrir Telegram y hacer /start
            </a>
            <div class="btn-close" onclick="window.close(); alert('Puedes cerrar esta pestaña y volver a Telegram.');">
                ❌ Puedes cerrar esta ventana
            </div>
        </div>
    </div>

    <script>
        const terminal = document.getElementById('terminal');
        const actions = document.getElementById('actions');
        const statusDot = document.getElementById('statusDot');
        const statusTitle = document.getElementById('statusTitle');
        const statusSubtitle = document.getElementById('statusSubtitle');
        const telegramBtn = document.getElementById('telegramBtn');

        function appendLog(msg, type = 'success') {
            const p = document.createElement('p');
            p.className = type;
            const time = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
            p.innerText = `[${time}] ${msg}`;
            terminal.appendChild(p);
            terminal.scrollTop = terminal.scrollHeight;
        }

        const evtSource = new EventSource('/stream-setup');

        evtSource.onmessage = function(event) {
            try {
                const data = JSON.parse(event.data);
                if (data.type === 'log') {
                    appendLog(data.message, data.style || 'success');
                } else if (data.type === 'complete') {
                    appendLog('🎉 ¡TODO LISTO! El Bot ya está activo y funcionando.', 'success');
                    statusDot.classList.add('active');
                    statusTitle.innerText = '¡Todo Listo!';
                    statusSubtitle.innerText = 'Bot Operativo';
                    
                    if (data.bot_username) {
                        telegramBtn.href = `https://t.me/${data.bot_username}`;
                        telegramBtn.innerText = `📲 Ir a Telegram (@${data.bot_username}) y enviar /start`;
                    }
                    
                    actions.style.display = 'flex';
                    evtSource.close();
                } else if (data.type === 'error') {
                    appendLog(`❌ ${data.message}`, 'error');
                    statusTitle.innerText = 'Error en configuración';
                    evtSource.close();
                }
            } catch(e) {
                console.error(e);
            }
        };

        evtSource.onerror = function() {
            evtSource.close();
        };
    </script>
</body>
</html>
"""


class SetupRequestHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/stream-setup":
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "keep-alive")
            self.end_headers()

            def send_event(event_type, msg, style="success", bot_username=""):
                payload = json.dumps({"type": event_type, "message": msg, "style": style, "bot_username": bot_username})
                try:
                    self.wfile.write(f"data: {payload}\n\n".encode("utf-8"))
                    self.wfile.flush()
                except Exception:
                    pass

            token = LATEST_CONFIG.get("token", "")
            user_id = LATEST_CONFIG.get("user_id", "")

            send_event("log", "⚙️ Guardando variables de entorno (.env) y cookies...", "info")
            time.sleep(0.5)

            bot_username = ""
            if token:
                send_event("log", "🔑 Verificando Token con Telegram Bot API...", "info")
                time.sleep(0.5)
                try:
                    req = urllib.request.Request(f"https://api.telegram.org/bot{token}/getMe")
                    with urllib.request.urlopen(req, timeout=5) as response:
                        res_data = json.loads(response.read().decode("utf-8"))
                        if res_data.get("ok"):
                            bot_info = res_data.get("result", {})
                            bot_username = bot_info.get("username", "")
                            bot_first_name = bot_info.get("first_name", "Bot")
                            send_event("log", f"✅ Bot Autenticado: {bot_first_name} (@{bot_username})", "success")
                except Exception as e:
                    send_event("log", f"⚠️ Aviso de API: No se pudo verificar @getMe ({e})", "error")

            if token and user_id:
                send_event("log", f"📩 Enviando mensaje de bienvenida automático a Telegram (ID: {user_id})...", "info")
                time.sleep(0.5)
                try:
                    welcome_text = (
                        "🚀 **¡Bot Multidescargas Configurado y Listo!** 🎉\n\n"
                        "Tu servidor web de inicio ha completado la instalación.\n\n"
                        "📌 **¿Cómo empezar?**\n"
                        "• Envía cualquier enlace (YouTube, TikTok, Instagram, Spotify...) para descargar.\n"
                        "• Usa `/help` para ver la lista de comandos disponibles."
                    )
                    url_msg = f"https://api.telegram.org/bot{token}/sendMessage"
                    data_msg = urllib.parse.urlencode({"chat_id": user_id, "text": welcome_text, "parse_mode": "Markdown"}).encode("utf-8")
                    req_msg = urllib.request.Request(url_msg, data=data_msg)
                    with urllib.request.urlopen(req_msg, timeout=5) as resp_msg:
                        send_event("log", "✅ Mensaje de bienvenida enviado con éxito a Telegram", "success")
                except Exception as e:
                    send_event("log", f"⚠️ No se pudo enviar el mensaje directo: {e}", "error")

            time.sleep(0.5)
            send_event("log", "🚀 Arrancando Bot Multidescargas en segundo plano...", "info")
            time.sleep(0.5)
            send_event("complete", "🎉 ¡Todo listo!", "success", bot_username)

            # Programar apagado inmediato para dar paso a main.py
            def shutdown():
                time.sleep(1.5)
                os._exit(0)

            threading.Thread(target=shutdown, daemon=True).start()
            return

        # Página principal con el formulario
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.end_headers()
        self.wfile.write(HTML_FORM_TEMPLATE.encode("utf-8"))

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length).decode("utf-8")
        parsed = urllib.parse.parse_qs(body)

        token = parsed.get("telegram_token", [""])[0].strip()
        user_id = parsed.get("allowed_user_id", [""])[0].strip()
        insta_user = parsed.get("instagram_user", [""])[0].strip()
        insta_pass = parsed.get("instagram_password", [""])[0].strip()
        dl_path = parsed.get("download_path", ["./downloads"])[0].strip() or "./downloads"
        cookies_txt = parsed.get("cookies", [""])[0].strip()

        if token and user_id:
            LATEST_CONFIG["token"] = token
            LATEST_CONFIG["user_id"] = user_id
            LATEST_CONFIG["insta_user"] = insta_user
            LATEST_CONFIG["insta_pass"] = insta_pass
            LATEST_CONFIG["cookies_txt"] = cookies_txt

            # Guardar .env
            env_content = f"TELEGRAM_TOKEN={token}\nALLOWED_USER_ID={user_id}\nDOWNLOAD_PATH={dl_path}\n"
            if insta_user:
                env_content += f"INSTAGRAM_USER={insta_user}\n"
            if insta_pass:
                env_content += f"INSTAGRAM_PASSWORD={insta_pass}\n"

            env_path = os.path.join(os.path.dirname(__file__), ".env")
            with open(env_path, "w", encoding="utf-8") as f:
                f.write(env_content)

            # Guardar cookies.txt si se especificó
            if cookies_txt:
                cookies_path = os.path.join(os.path.dirname(__file__), "cookies.txt")
                with open(cookies_path, "w", encoding="utf-8") as f:
                    f.write(cookies_txt)

            # Responder cargando la pantalla del terminal de logs en vivo
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write(HTML_TERMINAL_TEMPLATE.encode("utf-8"))
        else:
            self.send_response(400)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.end_headers()
            self.wfile.write("<h3>❌ Faltan datos obligatorios (Token o User ID). Vuelve atrás e inténtalo de nuevo.</h3>".encode("utf-8"))

    def log_message(self, format, *args):
        pass


def main():
    port = 8080
    server_address = ("", port)
    httpd = HTTPServer(server_address, SetupRequestHandler)
    print(f"🌐 Servidor web de configuración disponible en http://0.0.0.0:{port}")
    try:
        httpd.serve_forever()
    except (KeyboardInterrupt, SystemExit):
        print("🛑 Cerrando servidor web de configuración...")
        httpd.server_close()


if __name__ == "__main__":
    main()
