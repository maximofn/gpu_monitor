#!/usr/bin/env python3
import signal
import gi
gi.require_version('AppIndicator3', '0.1')
from gi.repository import AppIndicator3, GLib
from gi.repository import Gtk as gtk
import os
import pynvml
import webbrowser

APPINDICATOR_ID = 'GPU_monitor'

def main():
    path = os.path.dirname(os.path.realpath(__file__))
    icon_path = os.path.abspath(f"{path}/tarjeta-de-video.png")
    GPU_indicator = AppIndicator3.Indicator.new(APPINDICATOR_ID, icon_path, AppIndicator3.IndicatorCategory.SYSTEM_SERVICES)
    GPU_indicator.set_status(AppIndicator3.IndicatorStatus.ACTIVE)
    GPU_indicator.set_menu(build_menu())

    # Update GPU info every second
    GLib.timeout_add_seconds(1, update_gpu_info, GPU_indicator)

    GLib.MainLoop().run()

def update_gpu_info(indicator):
    device_count, gpu_info = get_gpu_info()

    info = ""
    for i in range(device_count):
        info += f" {i}->"
        if device_count > 1:
            info += f"{int(gpu_info[i]['memory_used'])}/"
            info += f"{int(gpu_info[i]['memory_total'])}MB->"
            info += f"{int(gpu_info[i]['temp'])}ºC"
        else:
            info += f"{gpu_info[i]['memory_used']}/"
            info += f"{gpu_info[i]['memory_total']}MB->"
            info += f"{gpu_info[i]['temp']}ºC"

    indicator.set_label(info, "Indicator")

    return True

def open_repo_link(_):
    webbrowser.open('https://github.com/maximofn/gpu_monitor')

def buy_me_a_coffe(_):
    webbrowser.open('https://www.buymeacoffee.com/maximofn')

def build_menu():
    menu = gtk.Menu()

    item_repo = gtk.MenuItem(label='Repository')
    item_repo.connect('activate', open_repo_link)
    menu.append(item_repo)

    item_buy_me_a_coffe = gtk.MenuItem(label='Buy me a coffe')
    item_buy_me_a_coffe.connect('activate', buy_me_a_coffe)
    menu.append(item_buy_me_a_coffe)

    item_quit = gtk.MenuItem(label='Quit')
    item_quit.connect('activate', quit)
    menu.append(item_quit)

    menu.show_all()
    return menu

def get_gpu_info():
    # Init NVML
    pynvml.nvmlInit()

    # Get number of devices
    device_count = pynvml.nvmlDeviceGetCount()

    gpu_info = {}

    for i in range(device_count):
        gpu_info[i] = {}

        # Obtener el identificador del dispositivo
        handle = pynvml.nvmlDeviceGetHandleByIndex(i)
        
        # Obtener la información de memoria
        memory_info = pynvml.nvmlDeviceGetMemoryInfo(handle)
        
        # Obtener la información de la temperatura
        temp = pynvml.nvmlDeviceGetTemperature(handle, pynvml.NVML_TEMPERATURE_GPU)

        memory_used = memory_info.used / 1024**2
        memory_total = memory_info.total / 1024**2

        gpu_info[i]["memory_used"] = memory_used
        gpu_info[i]["memory_total"] = memory_total
        gpu_info[i]["temp"] = temp
        
    # Finalizar NVML
    pynvml.nvmlShutdown()

    return device_count, gpu_info

if __name__ == "__main__":
    signal.signal(signal.SIGINT, signal.SIG_DFL) # Allow the program to be terminated with Ctrl+C
    main()
