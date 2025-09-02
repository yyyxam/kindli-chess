#!/bin/sh

# Debug-Info für MRPI Installation

LOG_FILE="/mnt/us/hellokindle/tmp/debug-info.log"

echo "=== MRPI DEBUG INFO ===" > "$LOG_FILE"
echo "Zeit: $(date)" >> "$LOG_FILE"

# 1. Aktuelle Position prüfen
echo "Aktueller Ordner: $(pwd)" >> "$LOG_FILE"

# 2. Unser Extension-Ordner
echo "" >> "$LOG_FILE"
echo "hello-test Ordner:" >> "$LOG_FILE"
ls -la /mnt/us/extensions/hellokindle/ >> "$LOG_FILE" 2>&1

# 3. Binary Check
echo "" >> "$LOG_FILE"
echo "Binary Check:" >> "$LOG_FILE"
if [ -f "/mnt/us/extensions/hello-kindle/bin/kindle-hello" ]; then
    echo "✅ Binary gefunden" >> "$LOG_FILE"
    file /mnt/us/hellokindle/kindle-hello >> "$LOG_FILE" 2>&1
else
    echo "❌ Binary NICHT gefunden" >> "$LOG_FILE"
fi

# 5. Permissions
echo "" >> "$LOG_FILE"
echo "File Permissions:" >> "$LOG_FILE"
ls -la /mnt/us/hellokindle/* >> "$LOG_FILE" 2>&1

# 6. System Info
echo "" >> "$LOG_FILE"
echo "System Info:" >> "$LOG_FILE"
uname -a >> "$LOG_FILE"
cat /proc/version >> "$LOG_FILE" 2>&1

# 7. Auf Display anzeigen
eips -c
eips -g "=== DEBUG INFO ==="
eips -g ""
eips -g "Log erstellt: /tmp/debug-info.log"
eips -g ""
if [ -f "/mnt/us/hellokindle/kindle-hello" ]; then
    eips -g "✅ Binary gefunden"
else
    eips -g "❌ Binary fehlt!"
fi
eips -g ""
eips -g "Über SSH/Terminal:"
eips -g "cat /mnt/us/hellokindle/tmp/debug-info.log"

sleep 5
eips -c
