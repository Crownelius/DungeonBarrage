import os
from collections import deque
import numpy as np
from PIL import Image

def get_rgba_and_fg(img_path):
    im = Image.open(img_path).convert('RGB')
    arr = np.array(im)
    h, w, _ = arr.shape
    is_white = np.all(arr >= 235, axis=2)
    bg_mask = np.zeros((h, w), dtype=bool)
    queue = deque()
    for x in range(w):
        if is_white[0, x]: queue.append((0, x)); bg_mask[0, x] = True
        if is_white[h-1, x]: queue.append((h-1, x)); bg_mask[h-1, x] = True
    for y in range(h):
        if is_white[y, 0]: queue.append((y, 0)); bg_mask[y, 0] = True
        if is_white[y, w-1]: queue.append((y, w-1)); bg_mask[y, w-1] = True
    while queue:
        y, x = queue.popleft()
        for dy, dx in [(-1,0), (1,0), (0,-1), (0,1)]:
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w:
                if not bg_mask[ny, nx] and is_white[ny, nx]:
                    bg_mask[ny, nx] = True
                    queue.append((ny, nx))
                    
    fg_mask = ~bg_mask
    rgba = np.zeros((h, w, 4), dtype=np.uint8)
    rgba[:, :, :3] = arr
    rgba[:, :, 3] = 255
    rgba[bg_mask, 3] = 0
    return rgba, fg_mask

def build_sheet(rgba, fg_mask, frame_boxes_by_row, out_path, num_rows=5, cell_w=192, cell_h=160, anchor_x=96, anchor_y=145, special_anchor=None):
    out = np.zeros((num_rows * cell_h, 5 * cell_w, 4), dtype=np.uint8)
    for r in range(num_rows):
        boxes = frame_boxes_by_row[r]
        for c in range(min(5, len(boxes))):
            x0, y0, x1, y1 = boxes[c]
            crop_rgba = rgba[y0:y1, x0:x1, :]
            crop_fg = fg_mask[y0:y1, x0:x1]
            ys, xs = np.where(crop_fg)
            if len(xs) == 0: continue
            
            sub_y0, sub_y1 = ys.min(), ys.max() + 1
            sub_x0, sub_x1 = xs.min(), xs.max() + 1
            sub = crop_rgba[sub_y0:sub_y1, sub_x0:sub_x1, :]
            sub_h, sub_w, _ = sub.shape
            
            if special_anchor:
                dest_top = special_anchor(r, c, sub_h, cell_h, anchor_y)
            else:
                dest_top = (r * cell_h) + anchor_y - sub_h
                
            dest_left = (c * cell_w) + anchor_x - (sub_w // 2)
            
            y_start = max(r * cell_h, dest_top)
            y_end = min((r + 1) * cell_h, dest_top + sub_h)
            x_start = max(c * cell_w, dest_left)
            x_end = min((c + 1) * cell_w, dest_left + sub_w)
            
            cy0 = y_start - dest_top
            cy1 = cy0 + (y_end - y_start)
            cx0 = x_start - dest_left
            cx1 = cx0 + (x_end - x_start)
            out[y_start:y_end, x_start:x_end] = sub[cy0:cy1, cx0:cx1]
            
    os.makedirs(os.path.dirname(out_path), exist_ok=True)
    Image.fromarray(out, 'RGBA').save(out_path)
    print(f'Generated: {out_path} ({out.shape[1]}x{out.shape[0]})')

def main():
    upload_dir = r'C:\Users\rsfit\.gemini\antigravity\brain\54030981-de17-4501-b984-fc07d4962b9e\.user_uploaded'
    out_dir = r'c:\Users\rsfit\DungeonBarrage\client\src\DungeonBarrage.Client\assets\sprites'
    os.makedirs(out_dir, exist_ok=True)

    # 1. Revolver
    print('Processing crow_revolver.png...')
    rev_rgba, rev_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487043936.jpg'))
    rev_boxes = [
        [(37, 20, 180, 155), (220, 20, 370, 155), (395, 20, 545, 155), (580, 20, 730, 155), (795, 20, 955, 155)],
        [(30, 180, 180, 315), (215, 180, 365, 315), (395, 180, 550, 315), (580, 180, 735, 315), (785, 180, 950, 315)],
        [(30, 330, 190, 460), (215, 330, 405, 460), (410, 330, 595, 460), (605, 330, 785, 460), (815, 330, 1005, 460)],
        [(35, 470, 165, 605), (220, 470, 360, 605), (405, 470, 540, 605), (600, 470, 750, 605), (810, 470, 955, 605)],
        [(30, 615, 170, 745), (215, 615, 360, 745), (395, 615, 540, 745), (585, 615, 725, 745), (795, 615, 940, 745)],
    ]
    build_sheet(rev_rgba, rev_fg, rev_boxes, os.path.join(out_dir, 'crow_revolver.png'))

    # 2. Pistol
    print('Processing crow_pistol.png...')
    pis_rgba, pis_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487043952.jpg'))
    pis_boxes = [
        [(35, 20, 170, 155), (220, 20, 355, 155), (395, 20, 535, 155), (580, 20, 725, 155), (800, 20, 940, 155)],
        [(30, 180, 170, 315), (215, 180, 360, 315), (395, 180, 540, 315), (580, 180, 730, 315), (785, 180, 940, 315)],
        [(30, 330, 180, 460), (215, 330, 400, 460), (405, 330, 595, 460), (605, 330, 785, 460), (820, 330, 985, 460)],
        [(35, 470, 170, 605), (220, 470, 355, 605), (405, 470, 545, 605), (595, 470, 735, 605), (790, 470, 935, 605)],
        [(30, 615, 165, 745), (220, 615, 350, 745), (395, 615, 525, 745), (585, 615, 715, 745), (795, 615, 935, 745)],
    ]
    build_sheet(pis_rgba, pis_fg, pis_boxes, os.path.join(out_dir, 'crow_pistol.png'))

    # 3. Bow
    print('Processing crow_bow.png...')
    bow_rgba, bow_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487043944.jpg'))
    bow_boxes = [
        [(35, 20, 160, 155), (215, 20, 335, 155), (390, 20, 510, 155), (580, 20, 730, 155), (795, 20, 935, 155)],
        [(30, 180, 170, 315), (215, 180, 350, 315), (390, 180, 530, 315), (575, 180, 715, 315), (780, 180, 920, 315)],
        [(35, 330, 180, 475), (220, 330, 365, 475), (400, 330, 560, 475), (595, 330, 770, 475), (800, 330, 990, 475)],
        [(40, 490, 210, 630), (225, 490, 415, 630), (450, 490, 650, 630), (695, 490, 805, 630), (840, 490, 990, 630)],
        [(30, 640, 155, 760), (205, 640, 325, 760), (370, 640, 490, 760), (535, 640, 645, 760), (690, 640, 795, 760)],
    ]
    def bow_anchor(r, c, sub_h, cell_h, anchor_y):
        if r == 3 and c == 3:
            return (r * cell_h) + anchor_y - 70 - (sub_h // 2)
        return (r * cell_h) + anchor_y - sub_h
    build_sheet(bow_rgba, bow_fg, bow_boxes, os.path.join(out_dir, 'crow_bow.png'), special_anchor=bow_anchor)

    # 4. Boomerang
    print('Processing crow_boomerang.png...')
    bm_rgba, bm_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487043974.jpg'))
    bm_boxes = [
        [(35, 20, 185, 155), (220, 20, 370, 155), (400, 20, 545, 155), (575, 20, 725, 155), (800, 20, 955, 155)],
        [(30, 180, 175, 315), (220, 180, 360, 315), (400, 180, 545, 315), (580, 180, 730, 315), (785, 180, 935, 315)],
        [(35, 330, 180, 465), (215, 330, 365, 465), (405, 330, 565, 465), (605, 330, 785, 465), (825, 330, 1000, 465)],
        [(25, 480, 170, 605), (190, 480, 330, 605), (340, 480, 470, 605), (515, 480, 615, 605), (875, 480, 1000, 605)],
        [(35, 620, 165, 745), (205, 620, 330, 745), (370, 620, 510, 745), (535, 620, 660, 745), (680, 620, 805, 745)],
    ]
    def bm_anchor(r, c, sub_h, cell_h, anchor_y):
        if r == 3 and (c == 2 or c == 3):
            return (r * cell_h) + anchor_y - 70 - (sub_h // 2)
        return (r * cell_h) + anchor_y - sub_h
    build_sheet(bm_rgba, bm_fg, bm_boxes, os.path.join(out_dir, 'crow_boomerang.png'), special_anchor=bm_anchor)

    # 5. Ramshot Cannon
    print('Processing crow_ramshot_cannon.png...')
    rc_rgba, rc_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487915781.jpg'))
    rc_boxes = [
        [(35, 20, 205, 155), (220, 20, 385, 155), (395, 20, 570, 155), (580, 20, 755, 155), (790, 20, 975, 155)],
        [(30, 180, 205, 315), (215, 180, 390, 315), (395, 180, 575, 315), (575, 180, 760, 315), (785, 180, 975, 315)],
        [(25, 340, 215, 470), (215, 340, 480, 470), (475, 340, 560, 470), (565, 340, 770, 470), (795, 340, 1000, 470)],
        [(25, 475, 195, 615), (220, 475, 390, 615), (410, 475, 580, 615), (595, 475, 755, 615), (790, 475, 955, 615)],
        [(30, 615, 195, 745), (215, 615, 385, 745), (395, 615, 570, 745), (580, 615, 755, 745), (790, 615, 965, 745)],
    ]
    def rc_anchor(r, c, sub_h, cell_h, anchor_y):
        if r == 2 and c >= 2:
            return (r * cell_h) + anchor_y - 70 - (sub_h // 2)
        return (r * cell_h) + anchor_y - sub_h
    build_sheet(rc_rgba, rc_fg, rc_boxes, os.path.join(out_dir, 'crow_ramshot_cannon.png'), special_anchor=rc_anchor)

    # 6. Mole Drill
    print('Processing crow_drill.png...')
    dr_rgba, dr_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487998769.jpg'))
    dr_boxes = [
        [(25, 15, 210, 155), (210, 15, 390, 155), (390, 15, 570, 155), (570, 15, 750, 155), (760, 15, 945, 155)],
        [(25, 160, 180, 340), (210, 160, 380, 340), (420, 160, 580, 340), (610, 160, 760, 340), (800, 160, 960, 340)],
        [(25, 345, 220, 485), (220, 345, 412, 485), (412, 345, 608, 485), (608, 345, 802, 485), (802, 345, 1010, 485)],
        [(20, 485, 150, 630), (185, 485, 380, 630), (405, 485, 595, 630), (615, 485, 810, 630), (820, 485, 975, 630)],
        [(20, 630, 195, 760), (200, 630, 385, 760), (395, 630, 580, 760), (595, 630, 775, 760), (805, 630, 985, 760)],
    ]
    build_sheet(dr_rgba, dr_fg, dr_boxes, os.path.join(out_dir, 'crow_drill.png'))

    # 7. Cinder Weapon
    print('Processing crow_cinder.png...')
    ci_rgba, ci_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788488090106.jpg'))
    ci_boxes = [
        [(25, 10, 185, 135), (200, 10, 360, 135), (395, 10, 560, 135), (575, 10, 745, 135), (755, 10, 930, 135)],
        [(20, 175, 195, 305), (205, 175, 390, 305), (410, 175, 580, 305), (600, 175, 775, 305), (790, 175, 975, 305)],
        [(20, 340, 200, 465), (210, 340, 395, 465), (410, 340, 605, 465), (610, 340, 800, 465), (610, 340, 800, 465)],
        [(20, 490, 140, 585), (145, 490, 285, 585), (590, 500, 680, 570), (690, 500, 795, 570), (920, 490, 1005, 575)],
        [(20, 620, 130, 735), (135, 620, 245, 735), (495, 620, 635, 735), (660, 620, 810, 735), (835, 620, 1005, 735)],
    ]
    def ci_anchor(r, c, sub_h, cell_h, anchor_y):
        if r == 3 and (c == 2 or c == 3):
            return (r * cell_h) + anchor_y - 70 - (sub_h // 2)
        elif r == 3 and c == 4:
            return (r * cell_h) + anchor_y - 65 - (sub_h // 2)
        return (r * cell_h) + anchor_y - sub_h
    build_sheet(ci_rgba, ci_fg, ci_boxes, os.path.join(out_dir, 'crow_cinder.png'), special_anchor=ci_anchor)

    # 8. Heavy Flail
    print('Processing crow_flail.png...')
    fl_rgba, fl_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487809781.jpg'))
    fl_boxes = [
        [(35, 20, 190, 155), (215, 20, 370, 155), (400, 20, 560, 155), (580, 20, 740, 155), (785, 20, 950, 155)],
        [(30, 180, 200, 315), (215, 180, 385, 315), (405, 180, 575, 315), (590, 180, 770, 315), (800, 180, 990, 315)],
        [(25, 315, 175, 470), (210, 315, 360, 470), (390, 315, 580, 470), (590, 315, 785, 470), (810, 315, 1010, 470)],
        [(25, 480, 205, 610), (220, 480, 420, 610), (410, 480, 615, 610), (630, 480, 820, 610), (845, 480, 1005, 610)],
        [(25, 620, 180, 745), (215, 620, 370, 745), (395, 620, 550, 745), (585, 620, 745, 745), (785, 620, 940, 745)],
    ]
    build_sheet(fl_rgba, fl_fg, fl_boxes, os.path.join(out_dir, 'crow_flail.png'))

    # 9. Pickaxe
    print('Processing crow_pickaxe.png...')
    pk_rgba, pk_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487739051.jpg'))
    pk_boxes = [
        [(35, 15, 175, 150), (215, 15, 355, 150), (395, 15, 535, 150), (575, 15, 735, 150), (800, 15, 960, 150)],
        [(30, 170, 175, 305), (215, 170, 360, 305), (395, 170, 550, 305), (580, 170, 740, 305), (790, 170, 945, 305)],
        [(30, 310, 175, 465), (215, 310, 365, 465), (400, 310, 555, 465), (600, 310, 740, 465), (820, 310, 965, 465)],
        [(30, 475, 175, 610), (195, 475, 335, 610), (360, 475, 505, 610), (525, 475, 670, 610), (670, 475, 840, 610)],
        [(35, 620, 170, 745), (200, 620, 335, 745), (360, 620, 495, 745), (525, 620, 655, 745), (685, 620, 815, 745)],
    ]
    build_sheet(pk_rgba, pk_fg, pk_boxes, os.path.join(out_dir, 'crow_pickaxe.png'))

    # 10. Damage
    print('Processing crow_damage.png...')
    dmg_rgba, dmg_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487151288.jpg'))
    dmg_boxes = [
        [(35, 60, 160, 210), (210, 60, 350, 210), (390, 60, 545, 210), (575, 60, 740, 210), (760, 60, 935, 210)],
        [(25, 250, 165, 395), (220, 250, 390, 395), (410, 250, 560, 395), (600, 250, 755, 395), (775, 250, 940, 395)],
        [(25, 420, 170, 565), (210, 420, 350, 565), (400, 420, 590, 565), (625, 420, 775, 565), (815, 420, 975, 565)],
        [(25, 585, 155, 730), (200, 585, 330, 730), (385, 585, 515, 730), (560, 585, 690, 730), (855, 585, 985, 730)],
    ]
    build_sheet(dmg_rgba, dmg_fg, dmg_boxes, os.path.join(out_dir, 'crow_damage.png'), num_rows=4)

    # 11. Flight
    print('Processing crow_flight.png...')
    flt_rgba, flt_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487291182.jpg'))
    flt_boxes = [
        [(35, 30, 175, 215), (215, 30, 380, 215), (420, 30, 580, 215), (625, 30, 785, 215), (835, 30, 995, 215)],
        [(25, 230, 195, 380), (220, 230, 400, 380), (420, 230, 600, 380), (620, 230, 800, 380), (820, 230, 1005, 380)],
        [(30, 400, 185, 545), (230, 400, 375, 545), (435, 400, 590, 545), (635, 400, 800, 545), (850, 400, 985, 545)],
        [(20, 570, 195, 730), (220, 570, 400, 730), (430, 570, 595, 730), (640, 570, 785, 730), (850, 570, 990, 730)],
    ]
    def flt_anchor(r, c, sub_h, cell_h, anchor_y):
        return (r * cell_h) + ((cell_h - sub_h) // 2)
    build_sheet(flt_rgba, flt_fg, flt_boxes, os.path.join(out_dir, 'crow_flight.png'), num_rows=4, special_anchor=flt_anchor)

    # 12. Potion
    print('Processing crow_potion.png...')
    pot_rgba, pot_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788487407636.jpg'))
    pot_boxes = [
        [(35, 20, 175, 160), (210, 20, 370, 160), (560, 20, 710, 160), (725, 20, 865, 160), (870, 20, 1010, 160)],
        [(40, 195, 190, 335), (220, 195, 370, 335), (405, 195, 555, 335), (580, 195, 730, 335), (750, 195, 890, 335)],
        [(35, 355, 175, 510), (220, 355, 355, 510), (400, 355, 570, 510), (590, 355, 735, 510), (755, 355, 930, 510)],
        [(35, 550, 175, 695), (215, 550, 370, 695), (390, 550, 530, 695), (570, 550, 705, 695), (760, 550, 895, 695)],
    ]
    build_sheet(pot_rgba, pot_fg, pot_boxes, os.path.join(out_dir, 'crow_potion.png'), num_rows=4)

    # 13. Frostfall Mortar
    print('Processing crow_frostfall.png...')
    ff_rgba, ff_fg = get_rgba_and_fg(os.path.join(upload_dir, 'media_1788488428730.png'))
    ff_rows_meta = [
        (12, 92, [(160, 264), (326, 429), (493, 596), (649, 752)]),
        (106, 178, [(160, 256), (330, 425), (497, 593), (652, 749)]),
        (190, 260, [(156, 270), (324, 436), (494, 604), (655, 766)]),
        (274, 343, [(143, 292), (312, 459), (481, 627), (648, 793)]),
        (358, 433, [(146, 247), (329, 433), (501, 599), (654, 756)]),
    ]
    ff_out = np.zeros((5 * 160, 5 * 192, 4), dtype=np.uint8)
    ff_scale = 1.75
    for r, (ys, ye, cols) in enumerate(ff_rows_meta):
        frame_indices = [0, 1, 2, 3, 2] if r < 2 else [0, 1, 2, 3, 3]
        for c, f_idx in enumerate(frame_indices):
            x0, x1 = cols[f_idx]
            crop_rgba = ff_rgba[ys:ye, x0:x1, :]
            crop_fg = ff_fg[ys:ye, x0:x1]
            c_ys, c_xs = np.where(crop_fg)
            if len(c_xs) == 0: continue
            sub = crop_rgba[c_ys.min():c_ys.max()+1, c_xs.min():c_xs.max()+1, :]
            pil_sub = Image.fromarray(sub, 'RGBA')
            new_w = int(pil_sub.width * ff_scale)
            new_h = int(pil_sub.height * ff_scale)
            pil_sub = pil_sub.resize((new_w, new_h), Image.NEAREST)
            scaled_sub = np.array(pil_sub)
            dest_top = (r * 160) + 145 - new_h
            dest_left = (c * 192) + 96 - (new_w // 2)
            y_start = max(r * 160, dest_top)
            y_end = min((r + 1) * 160, dest_top + new_h)
            x_start = max(c * 192, dest_left)
            x_end = min((c + 1) * 192, dest_left + new_w)
            cy0 = y_start - dest_top
            cy1 = cy0 + (y_end - y_start)
            cx0 = x_start - dest_left
            cx1 = cx0 + (x_end - x_start)
            ff_out[y_start:y_end, x_start:x_end] = scaled_sub[cy0:cy1, cx0:cx1]
    Image.fromarray(ff_out, 'RGBA').save(os.path.join(out_dir, 'crow_frostfall.png'))
    print(f'Generated: {os.path.join(out_dir, "crow_frostfall.png")} (960x800)')

    print('\nAll 13 character and weapon spritesheets successfully generated!')

if __name__ == '__main__':
    main()
