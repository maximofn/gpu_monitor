# GPU Monitor

Real-time GPU status tracking right on your Ubuntu menu bar.

![gpu monitor](https://maximofn.com/wp-content/uploads/2023/11/gpu_monitor.png)

## About GPU Monitor
GPU Monitor is an intuitive tool designed for developers, gamers, and professionals who need to keep an eye on their graphics card's performance and health in real time. It integrates seamlessly with the Ubuntu menu bar, providing essential information at your fingertips.

## Key Features
 * Real-time Monitoring: View GPU utilization, memory and temperature, all updated live.
 * Multi-GPU Support: Manage and monitor multiple GPUs from a single instance.
 * Optimized for Ubuntu: Crafted to integrate flawlessly with the latest Ubuntu OS.

## Installation

### Clone the repository

```bash
git clone https://github.com/maximofn/gpu_monitor.git
```

or with `ssh`

```bash
git clone git@github.com:maximofn/gpu_monitor.git
```

### Install the dependencies

Make sure that you do not have any `venv` or `conda` environment installed.

```bash
if [ -n "$VIRTUAL_ENV" ]; then
    deactivate
fi
if command -v conda &>/dev/null; then
    conda deactivate
fi
deactivate
```

Now install the dependencies

```bash
sudo apt-get install python3-gi python3-gi-cairo gir1.2-gtk-3.0
sudo apt-get install gir1.2-appindicator3-0.1
pip install nvidia-ml-py3
pip install pynvml
```

## Execution at start-up

Opens the application menu

![application menu](https://maximofn.com/wp-content/uploads/2023/11/applications_menu.png)

Type "start"

![start](https://maximofn.com/wp-content/uploads/2023/11/applications_at_startup.png)

Click on "Startup Applications"

![startup applications](https://maximofn.com/wp-content/uploads/2023/11/startup-application.webp)

Select "Add" and write the following:

 * Name: GPU Monitor
 * Command: /usr/bin/python3 path_of_script/gpu_monitor.py
 * Comment: GPU Monitor

And click on "Add"

## Support

Consider giving a **☆ Star** to this repository, if you also want to invite me for a coffee, click on the following button

[![BuyMeACoffee](https://img.shields.io/badge/Buy_Me_A_Coffee-support_my_work-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=white&labelColor=101010)](https://www.buymeacoffee.com/maximofn)