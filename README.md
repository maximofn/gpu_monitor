# GPU Monitor

🖥️ GPU Monitor for Ubuntu: The Ultimate Real-Time GPU Tracking Tool. Monitor your GPU's performance, temperature, and memory usage directly from your Ubuntu menu bar with GPU Monitor. This user-friendly and efficient application supports multiple GPUs and is fully integrated with the latest Ubuntu operating system. Get live updates and optimize your gaming or development tasks. Download now and take control of your GPU's health today!

![gpu monitor](gpu_monitor.gif)

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
```

Now install the dependencies

```bash
sudo apt-get install python3-gi python3-gi-cairo gir1.2-gtk-3.0
sudo apt-get install gir1.2-appindicator3-0.1
pip3 install nvidia-ml-py3
pip3 install pynvml
```

## Execution at start-up

```bash
add_to_startup.sh
```

Then when you restart your computer, the GPU Monitor will start automatically.

## Support

Consider giving a **☆ Star** to this repository, if you also want to invite me for a coffee, click on the following button

[![BuyMeACoffee](https://img.shields.io/badge/Buy_Me_A_Coffee-support_my_work-FFDD00?style=for-the-badge&logo=buy-me-a-coffee&logoColor=white&labelColor=101010)](https://www.buymeacoffee.com/maximofn)