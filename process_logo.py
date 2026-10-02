from PIL import Image
try:
    from rembg import remove
except ImportError:
    import time
    time.sleep(15)
    from rembg import remove

import io

input_path = r'C:\Users\Badhon\.gemini\antigravity-ide\brain\9c78b46d-9e89-4f48-a524-da908186508e\flashtransfer_logo_minimal_1790920779305.jpg'
output_path = r'd:\Personal Project\FlashTransfer\frontend\public\logo.png'

print('Opening image...')
input_img = Image.open(input_path)

print('Removing background...')
foreground = remove(input_img)

print('Creating solid clean box background...')
bg = Image.new('RGBA', foreground.size, (15, 20, 28, 255))
bg.paste(foreground, (0, 0), foreground)

print('Saving clean logo...')
bg.save(output_path, 'PNG')
print('Successfully created clean single-box logo!')
