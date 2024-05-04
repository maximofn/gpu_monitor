#!/usr/bin/env python3
import signal
import gi
gi.require_version('AppIndicator3', '0.1')
from gi.repository import AppIndicator3, GLib
from gi.repository import Gtk as gtk
import os
import pynvml
import webbrowser
import matplotlib.pyplot as plt
from PIL import Image, ImageDraw, ImageFont
import re
import time
import argparse
import subprocess
import io

APPINDICATOR_ID = 'GPU_monitor'

BLUE_COLOR = '#66b3ff'
RED_COLOR = '#ff6666'
GREEN_COLOR = '#99ff99'
ORANGE_COLOR = '#ffcc99'
YELLOW_COLOR = '#ffdb4d'
WHITE_COLOR = (255, 255, 255, 255)

PERCENTAGE_WARNING1 = 70
PERCENTAGE_WARNING2 = 80
PERCENTAGE_CAUTION = 90

PATH = os.path.dirname(os.path.realpath(__file__))
GPU_ICON_PATH = os.path.abspath(f"{PATH}/tarjeta-de-video.png")
FONT_PATH = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"

GPU_ICON = Image.open(GPU_ICON_PATH)

ICON_HEIGHT = 22
PADDING = 10

FONT_SIZE_FACTOR = 0.50
FONT_WIDTH_FACTOR = 7

image_to_show = None
old_image_to_show = None

GUI_GPU_indicator = None
GPU_temperature_menu_item = None
GPU_memory_used_menu_item = None
GPU_memory_free_menu_item = None
GPU_memory_total_menu_item = None
GPU_process_menu_items = None
actual_time_menu_item = None

def main(debug = False):
    global GUI_GPU_indicator

    GUI_GPU_indicator = AppIndicator3.Indicator.new(APPINDICATOR_ID, GPU_ICON_PATH, AppIndicator3.IndicatorCategory.SYSTEM_SERVICES)
    GUI_GPU_indicator.set_status(AppIndicator3.IndicatorStatus.ACTIVE)
    GUI_GPU_indicator.set_menu(build_menu(debug))

    # Update GPU info every second
    GLib.timeout_add_seconds(1, update_gpu_info, GUI_GPU_indicator, debug)

    GLib.MainLoop().run()

def update_gpu_info(indicator, debug = False):
    global image_to_show
    global old_image_to_show

    # Generate GPU info icon
    device_count, gpu_info = get_gpu_info(debug)
    
    # Update icon
    if not debug:
        info_icon_path = os.path.abspath(f"{PATH}/{image_to_show}")
        indicator.set_icon_full(info_icon_path, "GPU Usage")

    # Update old_image_to_show
    old_image_to_show = image_to_show

    # Update menu
    update_menu(device_count, gpu_info)

    return True

def open_repo_link(_):
    webbrowser.open('https://github.com/maximofn/gpu_monitor')

def buy_me_a_coffe(_):
    webbrowser.open('https://www.buymeacoffee.com/maximofn')

def build_menu(debug = False):
    global GPU_temperature_menu_item
    global GPU_memory_used_menu_item
    global GPU_memory_free_menu_item
    global GPU_memory_total_menu_item
    global GPU_process_menu_items
    global actual_time_menu_item

    menu = gtk.Menu()

    device_count, gpu_info = get_gpu_info(debug)

    GPU_temperature_menu_item = list(range(device_count))
    GPU_memory_used_menu_item = list(range(device_count))
    GPU_memory_free_menu_item = list(range(device_count))
    GPU_memory_total_menu_item = list(range(device_count))
    GPU_process_menu_items = {}

    for i in range(device_count):
        GPU_temperature_menu_item[i] = gtk.MenuItem(label=f"GPU {i} Temp: {gpu_info[i]['temp']}ºC")
        menu.append(GPU_temperature_menu_item[i])

        GPU_memory_used_menu_item[i] = gtk.MenuItem(label=f"GPU {i} Memory used {gpu_info[i]['memory_used']:.2f} MB")
        menu.append(GPU_memory_used_menu_item[i])

        GPU_memory_free_menu_item[i] = gtk.MenuItem(label=f"GPU {i} Memory free {gpu_info[i]['memory_total'] - gpu_info[i]['memory_used']:.2f} MB")
        menu.append(GPU_memory_free_menu_item[i])

        GPU_memory_total_menu_item[i] = gtk.MenuItem(label=f"GPU {i} Memory total {gpu_info[i]['memory_total']:.2f} MB")
        menu.append(GPU_memory_total_menu_item[i])

        horizontal_separator1 = gtk.SeparatorMenuItem()
        menu.append(horizontal_separator1)

        gpu_number_i_process_items = []
        for proc in gpu_info[i]['processes']:
            proc_item = gtk.MenuItem(label=f"GPU {i} - PID {proc['pid']} ({proc['used_memory'] / 1024**2:.2f} MB):\t{proc['name']}")
            menu.append(proc_item)
            gpu_number_i_process_items.append(proc_item)
        GPU_process_menu_items[f"{i}"] = gpu_number_i_process_items

        horizontal_separator2 = gtk.SeparatorMenuItem()
        menu.append(horizontal_separator2)

    actual_time_menu_item = gtk.MenuItem(label=time.strftime("%H:%M:%S"))
    menu.append(actual_time_menu_item)

    horizontal_separator3 = gtk.SeparatorMenuItem()
    menu.append(horizontal_separator3)
    
    item_repo = gtk.MenuItem(label='Repository')
    item_repo.connect('activate', open_repo_link)
    menu.append(item_repo)

    item_buy_me_a_coffe = gtk.MenuItem(label='Buy me a coffe')
    item_buy_me_a_coffe.connect('activate', buy_me_a_coffe)
    menu.append(item_buy_me_a_coffe)

    horizontal_separator4 = gtk.SeparatorMenuItem()
    menu.append(horizontal_separator4)

    item_quit = gtk.MenuItem(label='Quit')
    item_quit.connect('activate', quit)
    menu.append(item_quit)

    menu.show_all()
    return menu

def update_menu(device_count, gpu_info):
    global GPU_process_menu_items
    global GUI_GPU_indicator
    
    for i in range(device_count):
        # Update memory info for this GPU
        GPU_temperature_menu_item[i].set_label(f"GPU {i} Temp: {gpu_info[i]['temp']}ºC")
        GPU_memory_used_menu_item[i].set_label(f"GPU {i} Memory used {gpu_info[i]['memory_used']:.2f} MB")
        GPU_memory_free_menu_item[i].set_label(f"GPU {i} Memory free {gpu_info[i]['memory_total'] - gpu_info[i]['memory_used']:.2f} MB")
        GPU_memory_total_menu_item[i].set_label(f"GPU {i} Memory total {gpu_info[i]['memory_total']:.2f} MB")

        # Update actual time menu item
        actual_time_menu_item.set_label(time.strftime("%H:%M:%S"))

        # Get number of processes in this GPU
        number_of_processes = len(gpu_info[i]['processes'])
        len_processes_in_menu = len(GPU_process_menu_items[f'{i}'])

        # If the number of processes has changed, build the menu again
        if number_of_processes != len_processes_in_menu:
            GUI_GPU_indicator.set_menu(build_menu(debug))

        # If the number of processes is the same, update the menu items
        else:
            for (j, proc) in enumerate(gpu_info[i]['processes']):
                GPU_process_menu_items[f"{i}"][j].set_label(f"GPU {i} - PID {proc['pid']} ({proc['used_memory'] / 1024**2:.2f} MB):\t{proc['name']}")

def get_gpu_info(debug = False):
    global image_to_show
    global old_image_to_show

    # Init NVML
    pynvml.nvmlInit()

    # Get number of devices
    device_count = pynvml.nvmlDeviceGetCount()

    # List to store GPU info
    gpu_info = list(range(device_count))

    # Resize GPU icon
    gpu_icon_relation = GPU_ICON.width / GPU_ICON.height
    gpu_icon_width = int(ICON_HEIGHT * gpu_icon_relation)
    scaled_gpu_icon = GPU_ICON.resize((gpu_icon_width, ICON_HEIGHT), Image.LANCZOS)

    # Build menu items for each GPU
    for i in range(device_count):
        # Init dict with GPU info
        gpu_info[i] = {}

        # Get GPU handle by index
        handle = pynvml.nvmlDeviceGetHandleByIndex(i)
        
        # Get memory info of the GPU
        memory_info = pynvml.nvmlDeviceGetMemoryInfo(handle)
        
        # Get temperature of the GPU
        temp = pynvml.nvmlDeviceGetTemperature(handle, pynvml.NVML_TEMPERATURE_GPU)

        # Calculate usage memory, free memory and total memory
        memory_used = memory_info.used / 1024**2
        memory_total = memory_info.total / 1024**2
        memory_free = memory_total - memory_used

        # Add GPU info to dict
        gpu_info[i]["memory_used"] = memory_used
        gpu_info[i]["memory_total"] = memory_total
        gpu_info[i]["memory_free"] = memory_free
        gpu_info[i]["temp"] = temp
        gpu_info[i]['processes'] = get_gpu_processes(handle, i)

        # Get memory usage to create pie chart
        memory_labels = 'Used', 'Free'
        memory_used_size = memory_used / memory_total * 100
        memory_free_size = memory_free / memory_total * 100
        memory_free_size = [memory_used_size, memory_free_size]
        percentage_of_use = memory_free_size[0]

        # Assign color to memory usage chart
        if percentage_of_use < PERCENTAGE_WARNING1:
            memory_used_color = GREEN_COLOR
        elif percentage_of_use >= PERCENTAGE_WARNING1 and percentage_of_use < PERCENTAGE_WARNING2:
            memory_used_color = YELLOW_COLOR
        elif percentage_of_use >= PERCENTAGE_WARNING2 and percentage_of_use < PERCENTAGE_CAUTION:
            memory_used_color = ORANGE_COLOR
        else:
            memory_used_color = RED_COLOR
        memory_total_color = BLUE_COLOR
        pie_chart_colors = [memory_used_color, memory_total_color]
        explode = (0.1, 0)  # Explode used memory

        # Create memory usage pie chart
        pie_chart_figure, ax = plt.subplots()
        ax.pie(
            memory_free_size,
            explode=explode,
            labels=memory_labels,
            colors=pie_chart_colors,
            autopct='%1.1f%%',
            startangle=90,
            pctdistance=0.85,
            counterclock=False,
            wedgeprops=dict(width=0.3, edgecolor='w')
        )

        # Draw a circle at the center of pie to make it look like a donut
        centre_circle = plt.Circle((0,0), 0.70, fc='none', edgecolor='none')
        pie_chart_figure = plt.gcf()
        pie_chart_figure.gca().add_artist(centre_circle)

        # Equal aspect ratio ensures that pie is drawn as a circle
        ax.axis('equal')
        plt.tight_layout()

        # Save pie chart
        if not debug: plt.savefig(f"{PATH}/gpu_chart_{i}.png", transparent=True)
        plt.close(pie_chart_figure)

        # Load pie chart as PIL image
        if not debug: gpu_chart = Image.open(f'{PATH}/gpu_chart_{i}.png')

        # Resize chart
        if not debug:
            chart_icon_relation = gpu_chart.width / gpu_chart.height
            chart_icon_width = int(ICON_HEIGHT * chart_icon_relation)
            scaled_gpu_chart = gpu_chart.resize((chart_icon_width, ICON_HEIGHT), Image.LANCZOS)

        # New image with GPU info, GPU number and GPU chart
        if not debug:
            i_str = str(f" GPU {i}({temp}ºC)")
            i_str_width = len(i_str) * FONT_WIDTH_FACTOR
            total_width = scaled_gpu_icon.width + i_str_width + scaled_gpu_chart.width
            combined_image = Image.new('RGBA', (total_width, ICON_HEIGHT + PADDING), (0, 0, 0, 0))  # Transparent background

        # Combine GPU info and GPU chart
        if not debug:
            gpu_info_position = (0, int(PADDING/2))
            combined_image.paste(scaled_gpu_icon, gpu_info_position)
            chart_position = (scaled_gpu_icon.width + i_str_width, int(PADDING/2))
            combined_image.paste(scaled_gpu_chart, chart_position, scaled_gpu_chart)

        # Create font object
        if not debug:
            draw = ImageDraw.Draw(combined_image)
            font_size = int(ICON_HEIGHT * FONT_SIZE_FACTOR)
            font = ImageFont.truetype(FONT_PATH, font_size)

        # Get position of text
        if not debug: text_position = (scaled_gpu_icon.width, int((ICON_HEIGHT + PADDING - font_size) / 2))

        # Draw text
        if not debug: draw.text(text_position, i_str, font=font, fill=WHITE_COLOR)

        # Update scaled_gpu_icon. Asign to combined_image without padding
        if not debug: scaled_gpu_icon = combined_image.crop((0, PADDING/2, total_width, ICON_HEIGHT + PADDING/2))

    # Save combined image
    if not debug:
        timestamp = int(time.time())
        if not debug: image_to_show = f'gpu_info_{timestamp}.png'
        combined_image.save(f'{PATH}/{image_to_show}')

    # Remove old image
    if os.path.exists(f'{PATH}/{old_image_to_show}'):
        os.remove(f'{PATH}/{old_image_to_show}')
        
    # Shutdown NVML
    pynvml.nvmlShutdown()

    return device_count, gpu_info

def get_gpu_processes(handle, gpu_number):
    processes = pynvml.nvmlDeviceGetComputeRunningProcesses(handle)
    process_info = []
    # if len(processes) == 0:
    if True:
        try:
            nvidia_smi_output = subprocess.check_output(['nvidia-smi', 'pmon', '-c', '1', '-s', 'm'], encoding='utf-8')
            lines = nvidia_smi_output.strip().split('\n')
            for line in lines:
                # La expresión regular coincide con las líneas que tienen datos de procesos
                match = re.search(r'^\s*(\d+)\s+(\d+)\s+(\w+[+]*\w*)\s+(\d+)\s+(\d+)\s+(.*)$', line)
                if match:
                    gpu_id = match.group(1)
                    if int(gpu_id) != gpu_number:
                        continue
                    pid = match.group(2)
                    # type = match.group(3)
                    mem_used = match.group(4)  # Memoria usada en MB
                    command = match.group(6)
                    process_info.append({'pid': pid, 'name': command.strip(), 'used_memory': int(mem_used) * 1024 * 1024})  # Convert MB to bytes
        except subprocess.CalledProcessError as e:
            print(f"Error executing nvidia-smi: {e}")
            process_info.append({'pid': 'Error', 'name': 'nvidia-smi failed', 'used_memory': 0})
    else:
        for proc in processes:
            try:
                process_name = pynvml.nvmlSystemGetProcessName(proc.pid)
                process_info.append({'pid': proc.pid, 'name': process_name, 'used_memory': proc.usedGpuMemory})
            except pynvml.NvmlException:
                process_info.append({'pid': proc.pid, 'name': 'Unknown', 'used_memory': proc.usedGpuMemory})
    return process_info

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='GPU Monitor')
    parser.add_argument('--debug', action='store_true', help='Debug mode')
    args = parser.parse_args()
    debug = args.debug

    if not os.path.exists(GPU_ICON_PATH):
        print(f"Error: {GPU_ICON_PATH} not found")
        exit(1)
    
    # Remove all gpu_info_*.png files
    if not debug:
        for file in os.listdir(PATH):
            if re.search(r'gpu_info_\d+.png', file):
                os.remove(f'{PATH}/{file}')

    # Find files with gpu_chart_*.png and delete them
    if not debug:
        for file in os.listdir(PATH):
            if re.search(r'gpu_chart_\d+.png', file):
                os.remove(f"{PATH}/{file}")
    
    signal.signal(signal.SIGINT, signal.SIG_DFL) # Allow the program to be terminated with Ctrl+C
    main(debug)
