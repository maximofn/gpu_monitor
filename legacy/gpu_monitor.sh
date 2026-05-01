#!/usr/bin/bash
# Get script path
SCRIPT_PATH=$(dirname "$0")

if [ "$#" -gt 0 ]; then
    exec /usr/bin/python3 "$SCRIPT_PATH/gpu_monitor.py" "$@"
fi

exec /usr/bin/python3 "$SCRIPT_PATH/gpu_monitor.py" >gpu_monitor.log 2>gpu_monitor_error.log
