#! /bin/bash

# Get user
USER=$(whoami)

# Get file path
SCRIPT_FULL_PATH=$(realpath $0)
SCRIPT_PATH=$(dirname $SCRIPT_FULL_PATH)

# Set autostart path
AUTOSTART_PATH="/home/$USER/.config/autostart"

# Create autostart folder if it doesn't exist
if [ ! -d "$AUTOSTART_PATH" ]; then
    mkdir -p $AUTOSTART_PATH
fi

# Variables
script_name="gpu_monitor.sh"
script_info="GPU Monitor"

# Create autostart file
touch $AUTOSTART_PATH/$script_name.desktop

# Write autostart file
echo "[Desktop Entry]" > $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Type=Application" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Exec=$SCRIPT_PATH/$script_name" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Hidden=false" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "NoDisplay=false" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "X-GNOME-Autostart-enabled=true" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Name[es_ES]=$script_info" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Name=$script_info" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Comment[es_ES]=$script_info" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
echo "Comment=$script_info" >> $AUTOSTART_PATH/cpu_monitor_test.sh.desktop
