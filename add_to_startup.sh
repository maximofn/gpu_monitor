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
languaje=$(locale | grep LANG= | cut -d= -f2 | cut -d. -f1)

# Create autostart file
touch $AUTOSTART_PATH/$script_name.desktop

# Write autostart file
echo "[Desktop Entry]" > $AUTOSTART_PATH/$script_name.desktop
echo "Type=Application" >> $AUTOSTART_PATH/$script_name.desktop
echo "Exec=$SCRIPT_PATH/$script_name" >> $AUTOSTART_PATH/$script_name.desktop
echo "Hidden=false" >> $AUTOSTART_PATH/$script_name.desktop
echo "NoDisplay=false" >> $AUTOSTART_PATH/$script_name.desktop
echo "X-GNOME-Autostart-enabled=true" >> $AUTOSTART_PATH/$script_name.desktop
echo "Name[$languaje]=$script_info" >> $AUTOSTART_PATH/$script_name.desktop
echo "Name=$script_info" >> $AUTOSTART_PATH/$script_name.desktop
echo "Comment[$languaje]=$script_info" >> $AUTOSTART_PATH/$script_name.desktop
echo "Comment=$script_info" >> $AUTOSTART_PATH/$script_name.desktop
