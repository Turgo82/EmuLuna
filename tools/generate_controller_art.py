"""Build EmuLuna's original vector controller illustrations and matching hit areas.

Coordinates describe the hardware, not widget pixels. Keep the artwork and hit
areas together so resizing can never separate a label from its physical button.
Run from any directory after changing an illustration. No third-party art used.
"""
import argparse
import json
import math
from pathlib import Path
from xml.sax.saxutils import escape

DEST = Path(__file__).resolve().parents[1] / 'emuluna/data/controllers'
DEST.mkdir(exist_ok=True)
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--only', nargs='+', choices=sorted(p.stem for p in DEST.glob('*.svg')),
                    help='Regenerate only these controllers, preserving other artwork and layouts.')
selected = parser.parse_args().only
CATALOG = json.loads((DEST/'layouts.json').read_text()) if selected else {}


class Art:
    def __init__(self, key, title, view=(600, 360), note=''):
        self.key, self.title, self.view, self.note = key, title, view, note
        self.shapes, self.controls = [], []
        self.shapes.append('''<defs>
          <linearGradient id="plastic" x2="0" y2="1"><stop stop-color="#ecece9"/><stop offset=".5" stop-color="#c5c7c9"/><stop offset="1" stop-color="#999da3"/></linearGradient>
          <linearGradient id="black" x2="0" y2="1"><stop stop-color="#45474b"/><stop offset=".5" stop-color="#26282c"/><stop offset="1" stop-color="#131519"/></linearGradient>
          <linearGradient id="purple" x2="0" y2="1"><stop stop-color="#7b77bf"/><stop offset="1" stop-color="#3f3b7b"/></linearGradient>
          <linearGradient id="glass" x2="1" y2="1"><stop stop-color="#667c80"/><stop offset=".45" stop-color="#293d43"/><stop offset="1" stop-color="#17282f"/></linearGradient>
          <linearGradient id="rubber" x2="0" y2="1"><stop stop-color="#51535a"/><stop offset="1" stop-color="#17191c"/></linearGradient>
          <linearGradient id="cap-light" x2=".3" y2="1"><stop stop-color="#ffffff" stop-opacity=".18"/><stop offset=".45" stop-color="#ffffff" stop-opacity="0"/><stop offset="1" stop-color="#000000" stop-opacity=".24"/></linearGradient>
        </defs>''')

    def rect(self, x, y, w, h, fill, radius=6, stroke='#111317', sw=2):
        self.shapes.append(f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{radius}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"/>')

    def circle(self, x, y, r, fill, stroke='#111317', sw=2):
        self.shapes.append(f'<circle cx="{x}" cy="{y}" r="{r}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"/>')

    def path(self, path, fill, stroke='#111317', sw=2):
        self.shapes.append(f'<path d="{path}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}" stroke-linejoin="round"/>')

    def text(self, x, y, text, color='#dddfe2', size=14):
        self.shapes.append(f'<text x="{x}" y="{y}" fill="{color}" font-family="sans-serif" font-size="{size}" font-weight="600" text-anchor="middle">{escape(text)}</text>')

    def hit(self, action, label, rect, group='Buttons', ellipse=False, rotation=0):
        if isinstance(action, int):
            action = f'button:{action}'
        if action:
            self.controls.append(dict(action=action, label=label, rect=rect,
                                      group=group, ellipse=ellipse))
            if rotation:
                self.controls[-1]['rotation'] = rotation

    def button(self, action, label, x, y, r=20, fill='#393c42', group='Buttons', ink='#f4f4f4'):
        self.circle(x, y + 2, r + 2, '#101115', sw=0)
        self.circle(x, y, r, fill, '#676970', 1.5)
        self.path(f'M {x-r*.65} {y-r*.5} Q {x} {y-r*1.2} {x+r*.65} {y-r*.5}', 'none', '#ffffff', .7)
        self.text(x, y + 5, label, ink, 15)
        self.hit(action, label, [x-r, y-r, 2*r, 2*r], group, True)

    def small(self, action, label, x, y, w=62, h=22, group='Menu buttons', fill='url(#rubber)'):
        self.rect(x, y, w, h, fill, h/2, '#5f6267', 1)
        self.text(x+w/2, y+h/2+4, label, size=11)
        self.hit(action, label, [x,y,w,h], group)

    def shoulders(self, left=512, right=256, labels=('L','R'), y=62):
        for action, label, x in ((left,labels[0],100),(right,labels[1],400)):
            self.small(action,label,x,y,100,25,'Shoulders & triggers')

    def dpad(self, x, y, size=28, bits=(64,128,32,16), labels=('↑','↓','←','→'), group='Directional pad'):
        s=size
        self.path(f'M {x-s/2} {y-s*1.5} h {s} v {s} h {s} v {s} h {-s} v {s} h {-s} v {-s} h {-s} v {-s} h {s} Z', 'url(#rubber)', '#777a81', 2)
        self.circle(x,y,s*.25,'#24262a','#121416',1)
        for bit,label,dx,dy in zip(bits,labels,(0,0,-s,s),(-s,s,0,0)):
            self.text(x+dx,y+dy+5,label,size=12)
            self.hit(bit,label,[x+dx-s/2,y+dy-s/2,s,s],group)

    def stick(self, x, y, axes=(0,1), click=None, r=36, digital=False, brass=False):
        self.circle(x,y,r+9,'#5e6267','#111318',2)
        self.circle(x,y,r,'#b7a370' if brass else 'url(#rubber)','#777b83',2)
        self.circle(x,y,r-7,'none','#dcc887' if brass else '#3d4149',2)
        if click:
            self.text(x,y+5,click[1],size=14)
            self.hit(click[0],click[1],[x-r/2,y-r/2,r,r],'Stick clicks',True)
        for i,(dx,dy) in enumerate(((0,-1),(0,1),(-1,0),(1,0))):
            action = (64,128,32,16)[i] if digital else f'axis:{axes[1] if dy else axes[0]}:{dy or dx}'
            self.hit(action,('↑','↓','←','→')[i],
                     [x+dx*r*.72-r*.3,y+dy*r*.72-r*.3,r*.6,r*.6],
                     'Directional pad' if digital else 'Analog stick')

    def screen(self, x, y, w, h, mono=False):
        self.rect(x-10,y-10,w+20,h+20,'#202328',9,'#7c7f84',1)
        self.rect(x,y,w,h,'#8a9b60' if mono else 'url(#glass)',1,'#11161a',2)
        self.path(f'M {x+2} {y+2} H {x+w-2} L {x+2} {y+h*.6} Z', '#ffffff', 'none', 0)
        # A subtle screen reflection, rather than an imitation game image.
        self.shapes[-1] = self.shapes[-1].replace('fill="#ffffff"','fill="#ffffff" opacity=".06"')

    def material(self, name, light, middle, dark):
        self.shapes.append(f'<linearGradient id="{name}" x2=".3" y2="1"><stop stop-color="{light}"/><stop offset=".5" stop-color="{middle}"/><stop offset="1" stop-color="{dark}"/></linearGradient>')
        return f'url(#{name})'

    def wire(self, x, y, length=27):
        self.path(f'M {x} {y-length} V {y+6}','none','#454750',7)
        self.path(f'M {x-1.4} {y-length} V {y}','none','#72747c',1)
        self.rect(x-6,y-8,12,18,'url(#rubber)',3,'#555861',1)
        for dy in (-4,0,4): self.path(f'M {x-5} {y+dy} H {x+5}','none','#777983',.7)

    def face(self, action, label, x, y, r=20, fill='#484a53', ink='#d8d9df', inside=False, label_pos=None, group='Buttons'):
        self.circle(x,y+1.5,r+2.5,'#13151b','#73757f',.8)
        self.circle(x,y,r,fill,'#9b9da7',.8)
        self.shapes.append(f'<circle cx="{x}" cy="{y}" r="{r-1}" fill="url(#cap-light)"/>')
        self.path(f'M {x-r*.66} {y-r*.52} Q {x} {y-r*1.1} {x+r*.66} {y-r*.52}','none','#bfc1cc',.65)
        if label:
            lx,ly = label_pos or (x,y+4 if inside else y+r+14)
            self.text(lx,ly,label,ink,max(8,min(14,r*.68)))
        self.hit(action,label,[x-r,y-r,2*r,2*r],group,True)

    def menu_key(self, action, label, x, y, w=40, h=14, angle=0, ink='#7c7f8c', fill='url(#rubber)', label_below=True):
        self.shapes.append(f'<g transform="rotate({angle} {x} {y})">')
        self.rect(x-w/2-2,y-h/2-2,w+4,h+4,'#34363e',h/2+2,'#8e909c',.8)
        self.rect(x-w/2,y-h/2,w,h,fill,h/2,'#737680',.8)
        self.shapes.append('</g>')
        if label:
            self.text(x,y+(abs(math.sin(math.radians(angle)))*w+h)/2+13 if label_below else y-h/2-8,label,ink,9)
        self.hit(action,label,[x-w/2-1,y-h/2-1,w+2,h+2],'Menu buttons',rotation=angle)

    def cross(self, x, y, s=24, bits=(64,128,32,16), group='Directional pad', fill='url(#rubber)', disc=False, split=False):
        if disc:
            self.circle(x,y,s*1.9,'#181a21','#777b88',1.3)
            self.circle(x,y,s*1.8,fill,'#4e5260',1)
        for i,(dx,dy) in enumerate(((0,-1),(0,1),(-1,0),(1,0))):
            self.hit(bits[i],('↑','↓','←','→')[i],[x+dx*s-s/2,y+dy*s-s/2,s,s],group)
        if split:
            for angle in (0,90,180,270):
                self.shapes.append(f'<g transform="rotate({angle} {x} {y})">')
                self.path(f'M {x-s*.46} {y-s*1.52} H {x+s*.46} V {y-s*.62} L {x} {y-s*.27} L {x-s*.46} {y-s*.62} Z',fill,'#6a6d78',1.3)
                self.shapes.append('</g>')
        else:
            self.path(f'M {x-s/2} {y-s*1.5} h {s} v {s} h {s} v {s} h {-s} v {s} h {-s} v {-s} h {-s} v {-s} h {s} Z',fill,'#656976',1.4)
            self.circle(x,y,s*.20,'#292c33','#444955',.6)
        # Molded direction marks are deliberately quiet, like the physical pad.
        for angle in (0,90,180,270):
            self.shapes.append(f'<g transform="rotate({angle} {x} {y})">')
            self.path(f'M {x} {y-s*1.25} l {-s*.21} {s*.28} h {s*.42} Z','#515560','#2b2e37',.6)
            self.shapes.append('</g>')

    def sega_dpad(self, x, y, r=53):
        """Circular rocker and molded cross used on the six-button Sega pads."""
        self.circle(x,y,r+9,'#13161d','#525965',1.2)
        self.circle(x,y,r+3,'#20252e','#10141b',1.4)
        self.circle(x,y,r,'url(#sega-rocker)','#646d7c',1)
        s=r*.27
        self.path(f'M {x-s} {y-r+3} Q {x} {y-r} {x+s} {y-r+3} L {x+s} {y-s} H {x+r-3} Q {x+r} {y} {x+r-3} {y+s} H {x+s} V {y+r-3} Q {x} {y+r} {x-s} {y+r-3} V {y+s} H {x-r+3} Q {x-r} {y} {x-r+3} {y-s} H {x-s} Z','url(#rubber)','#606978',1.3)
        self.circle(x,y,7,'#20252d','#4b5666',1)
        self.path(f'M {x-4} {y-4} Q {x} {y-7} {x+4} {y-4}','none','#78818f',.7)
        for bit,label,dx,dy,angle in ((64,'↑',0,-1,0),(128,'↓',0,1,180),(32,'←',-1,0,270),(16,'→',1,0,90)):
            self.shapes.append(f'<g transform="rotate({angle} {x} {y})">')
            self.path(f'M {x} {y-r+9} l -4 5 h 8 Z','#555e6e','#202733',.6)
            self.shapes.append('</g>')
            self.hit(bit,label,[x+dx*r*.63-14,y+dy*r*.63-14,28,28],'Directional pad')

    def finish(self):
        if selected and self.key not in selected:
            return
        w,h=self.view
        (DEST/f'{self.key}.svg').write_text(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}">'+''.join(self.shapes)+'</svg>\n')
        CATALOG[self.key] = dict(title=self.title, size=self.view, note=self.note, controls=self.controls)


# NES and Famicom share a button layout, but have distinct faceplates and colors.
for key,title,color in (('nes','NES controller','url(#plastic)'),('fds','Famicom controller','#b13e3b')):
    a=Art(key,title,(600,310))
    a.wire(173 if key=='nes' else 74,52,30)
    if key=='fds':
        color=a.material('famicom-shell','#c43545','#aa2031','#871626')
    a.rect(35,56,530,219,'#6c6c73',12,'#787b83',1)
    a.rect(35,51,530,219,color,12,'#9da0a8',1.5)
    if key=='nes':
        a.rect(48,64,504,192,'#24262c',5,'#84858b',1)
        for y in (72,96,120,194,220): a.rect(222,y,127,15,'#777b7e',2,'none',0)
        a.rect(219,161,134,43,'#b7b8b7',3,'none',0)
    else:
        gold=a.material('famicom-plate','#ead6a5','#d1b87d','#bca36f')
        a.path('M 60 64 H 219 Q 227 64 227 73 V 116 Q 227 126 237 126 H 540 Q 552 126 552 140 V 243 Q 552 256 539 256 H 60 Q 48 256 48 243 V 78 Q 48 64 60 64 Z',gold,'#8c713d',1)
        for y in (190,197): a.path(f'M 50 {y} H 550','none','#51482d',1.5)
        a.rect(215,168,140,37,color,18,'#e1b67e',1)
        a.rect(64,77,28,22,'#332e21',0,'none',0);a.text(78,94,'I','#eddbac',18)
    a.cross(131,170,27)
    for bit,label,x in ((4,'SELECT',251),(8,'START',320)):
        if key=='nes':
            a.menu_key(bit,label,x,182,42,15,ink='#b8444c',label_below=False)
        else:
            a.rect(x-20,177,40,17,'url(#rubber)',4,'#63646b',1)
            a.text(x,159,label,'#4a422c',9)
            a.hit(bit,label,[x-21,175,42,20],'Menu buttons')
    for bit,label,x in ((2,'B',407),(1,'A',491)):
        if key=='nes':
            a.rect(x-33,155,65,69,'#bebfbd',3,'none',0)
            a.face(bit,label,x,187,24,'#b72a38',ink='#c5535a',label_pos=(x+27,236))
        else:
            a.circle(x,191,27,'#a52635','#dbb77c',1)
            a.rect(x-14,142,28,14,'#302d23',7,'none',0)
            a.face(bit,label,x,189,23,'#292b31',ink='#ebd5a2',label_pos=(x,153))
    a.finish()

# North American pad: shallow waist, recessed cross, diagonal rubber keys,
# and concave lavender X/Y versus convex purple A/B. Labels are on the shell.
a=Art('snes','Super Nintendo controller',(600,310))
a.shapes.append('''<defs>
  <linearGradient id="snes-shell" x2="0" y2="1"><stop stop-color="#dedee3"/><stop offset=".52" stop-color="#cbccd3"/><stop offset="1" stop-color="#b4b5be"/></linearGradient>
  <linearGradient id="snes-rim" x2="0" y2="1"><stop stop-color="#b7b8c2"/><stop offset="1" stop-color="#858792"/></linearGradient>
  <linearGradient id="snes-shoulder" x2="0" y2="1"><stop stop-color="#d1d1db"/><stop offset=".6" stop-color="#aeb0bd"/><stop offset="1" stop-color="#868995"/></linearGradient>
  <radialGradient id="snes-dish" cx=".5" cy=".6" r=".65"><stop offset=".7" stop-color="#bfc0c9"/><stop offset="1" stop-color="#a3a5b0"/></radialGradient>
  <linearGradient id="snes-well" x2=".7" y2="1"><stop stop-color="#9899ab"/><stop offset="1" stop-color="#818395"/></linearGradient>
  <linearGradient id="snes-pairs" x2="0" y2="1"><stop stop-color="#d8d8e2"/><stop offset="1" stop-color="#b6b6c7"/></linearGradient>
  <radialGradient id="snes-concave" cx=".45" cy=".2" r=".85"><stop stop-color="#9486bd"/><stop offset=".65" stop-color="#ad9ed6"/><stop offset="1" stop-color="#c6b9e9"/></radialGradient>
  <radialGradient id="snes-convex" cx=".35" cy=".22" r=".85"><stop stop-color="#7260a3"/><stop offset=".65" stop-color="#594286"/><stop offset="1" stop-color="#44306b"/></radialGradient>
  <linearGradient id="snes-cross" x2="0" y2="1"><stop stop-color="#45464e"/><stop offset="1" stop-color="#2e2f36"/></linearGradient>
</defs>''')
# A short cord and molded strain relief identify the original wired controller.
a.path('M 300 9 V 48','none','#24252a',9)
a.path('M 298 9 V 47','none','#53545b',1.3)
a.rect(292,38,16,30,'#45464e',4,'#25262d',1)
for y in (43,49,55): a.path(f'M 293 {y} H 307','none','#6b6c75',1)
for bit,label,mirror in ((512,'L',False),(256,'R',True)):
    a.shapes.append('<g transform="translate(600 0) scale(-1 1)">' if mirror else '<g>')
    a.path('M 76 81 C 83 62 100 52 128 50 H 208 Q 217 50 214 58 L 208 68 L 101 86 Z','url(#snes-shoulder)','#797c89',1.2)
    a.path('M 94 64 Q 108 54 130 54 H 208','none','#e7e7ee',1)
    a.shapes.append('</g>')
    x=449 if mirror else 151
    a.text(x,58,label,'#737684',8)
    a.hit(bit,label,[x-42,50,84,13],'Shoulders & triggers')
shell='M 145 60 H 455 C 522 60 576 103 576 171 C 576 235 528 284 467 284 C 430 284 409 275 380 259 H 220 C 191 275 169 284 133 284 C 70 284 24 235 24 171 C 24 103 78 60 145 60 Z'
a.shapes.append('<g transform="translate(0 5)">')
a.path(shell,'url(#snes-rim)','#747783',1.5)
a.shapes.append('</g>')
a.path(shell,'url(#snes-shell)','#9698a4',1.5)
a.path('M 29 159 C 35 103 81 64 145 64 H 455 C 516 64 562 101 570 151','none','#f0f0f4',2)
# The D-pad sits in a shallow dish of the same plastic, not a dark face plate.
a.circle(143,177,67,'url(#snes-dish)','#acadb8',1)
a.path('M 80 194 A 66 66 0 0 0 206 193','none','#e3e3e9',1.5)
a.circle(464,176,97,'url(#snes-well)','#727583',1.2)
a.path('M 371 202 A 97 97 0 0 0 557 202','none','#dcdee7',1.5)
cross='M 129 133 H 157 V 163 H 187 V 191 H 157 V 221 H 129 V 191 H 99 V 163 H 129 Z'
a.path(cross,'#222329','#9495a0',7)
a.path(cross,'url(#snes-cross)','#53555f',1.8)
a.circle(143,177,9,'#383940','none',0)
# Low-contrast molded triangles, rather than printed arrow glyphs.
for bit,label,shape,rect in (
    (64,'↑','M 143 143 L 136 153 H 150 Z',[129,133,28,29]),
    (128,'↓','M 143 211 L 136 201 H 150 Z',[129,192,28,29]),
    (32,'←','M 109 177 L 119 170 V 184 Z',[99,163,29,28]),
    (16,'→','M 177 177 L 167 170 V 184 Z',[158,163,29,28])):
    a.path(shape,'#42444d','#282930',.8)
    a.hit(bit,label,rect,'Directional pad')
# Rubber Start/Select buttons rise diagonally; text is printed below each key.
for bit,label,cx in ((4,'SELECT',259),(8,'START',320)):
    a.shapes.append(f'<g transform="rotate(-35 {cx} 204)">')
    a.rect(cx-24,194,48,20,'#858792',10,'#b8b9c3',1)
    a.rect(cx-22,195,44,17,'#24252b',8.5,'#555762',1)
    a.rect(cx-20,195,40,14,'url(#rubber)',7,'#6b6e77',.7)
    a.shapes.append('</g>')
    a.shapes.append(f'<text x="{cx}" y="239" fill="#656775" font-family="sans-serif" font-size="11" font-weight="700" font-style="italic" letter-spacing="1" text-anchor="middle">{label}</text>')
    a.hit(bit,label,[cx-24,194,48,20],'Menu buttons',rotation=-35)
# Each diagonal pair has its own light plastic surround.
for cx,cy in ((438,155),(491,196)):
    a.shapes.append(f'<g transform="rotate(-38 {cx} {cy})">')
    a.rect(cx-59,cy-26,118,52,'url(#snes-pairs)',26,'#717481',1.2)
    a.shapes.append('</g>')
for bit,label,x,y,concave,lx,ly in (
    (1024,'X',464,134,True,497,117),
    (2048,'Y',412,175,True,379,203),
    (1,'A',516,175,False,548,159),
    (2,'B',464,216,False,431,246)):
    a.circle(x,y+1,23,'#5a576e','#9996b0',1)
    a.circle(x,y,21.5,'#c8bae7' if concave else '#806cab','#e3dcee' if concave else '#b5a4d0',.8)
    a.circle(x,y,19.5,'url(#snes-concave)' if concave else 'url(#snes-convex)','#8b7cae' if concave else '#524172',.7)
    a.path(f'M {x-14} {y+12} Q {x} {y+23} {x+14} {y+12}' if concave else f'M {x-14} {y-12} Q {x} {y-23} {x+14} {y-12}', 'none','#d9c9f0' if concave else '#ad99d0',.8)
    a.shapes.append(f'<text x="{lx}" y="{ly}" fill="#dbdbe6" font-family="sans-serif" font-size="16" font-style="italic" font-weight="600" text-anchor="middle">{label}</text>')
    a.hit(bit,label,[x-23,y-23,46,46],ellipse=True)
a.finish()

# The Genesis six-button pad and Saturn model 2 are distinct hardware designs.
# Keep the original button IDs: their libretro layouts differ despite the labels.
for key,title in (('genesis','Mega Drive / Genesis · six-button pad'),('saturn','Saturn · model 2 control pad')):
    a=Art(key,title,(600,350))
    shell_fill=a.material('sega-shell','#424650','#292d36','#1e222b')
    a.material('sega-rocker','#686f7a','#424955','#2a303b')
    a.shapes.append('''<defs>
      <linearGradient id="sega-panel" x2=".5" y2="1"><stop stop-color="#171b23"/><stop offset=".42" stop-color="#303642"/><stop offset=".65" stop-color="#161b24"/><stop offset="1" stop-color="#252c37"/></linearGradient>
      <linearGradient id="sega-silver" x2=".3" y2="1"><stop stop-color="#c6c9cd"/><stop offset=".5" stop-color="#999fa8"/><stop offset="1" stop-color="#757d88"/></linearGradient>
    </defs>''')
    if key=='genesis':
        a.wire(300,57,32)
        # Mode is on the top edge, not a second menu button on the face.
        a.rect(429,69,58,14,'url(#rubber)',5,'#757d89',1)
        a.text(458,79,'MODE','#b7bec9',7)
        a.hit(4,'MODE',[429,69,58,14],'Shoulders & triggers')
        outline='M 139 85 C 183 59 232 55 300 55 C 368 55 417 59 461 85 C 516 112 553 163 562 222 C 571 264 560 302 532 311 C 510 318 492 302 465 286 C 410 246 353 240 300 241 C 247 240 190 246 135 286 C 108 307 91 321 68 311 C 40 301 29 266 38 222 C 47 163 84 112 139 85 Z'
    else:
        a.wire(300,88,43)
        # Slim gray caps follow the top corners of the housing.
        for mirrored in (False,True):
            a.shapes.append('<g transform="translate(600 0) scale(-1 1)">' if mirrored else '<g>')
            a.path('M 73 114 Q 89 68 122 57 Q 153 45 195 61 L 190 74 Q 123 76 73 114 Z','url(#sega-silver)','#767e8a',1)
            a.path('M 92 86 Q 124 56 184 64','none','#d7d8dc',.9)
            a.shapes.append('</g>')
        for bit,label,x in ((4096,'L',146),(8192,'R',454)):
            a.text(x,67,label,'#4e5663',8)
            a.hit(bit,label,[x-34,53,68,22],'Shoulders & triggers')
        outline='M 137 71 C 194 63 233 94 300 94 C 367 94 406 63 463 71 C 501 77 524 91 543 120 C 573 167 581 221 567 271 C 559 303 547 319 524 321 C 493 324 469 302 443 287 C 399 273 349 268 300 268 C 251 268 201 273 157 287 C 131 302 107 324 76 321 C 53 318 37 303 33 271 C 19 221 27 167 57 120 C 76 91 99 77 137 71 Z'
    a.shapes.append('<g transform="translate(0 4)">')
    a.path(outline,'#171b23','#555d6c',1.4)
    a.shapes.append('</g>')
    a.path(outline,shell_fill,'#717988',1.2)
    if key=='genesis':
        a.path('M 154 77 Q 300 38 446 77 L 432 90 Q 300 62 168 90 Z','#363b45','#161c25',1)
        a.path('M 84 167 C 105 117 163 96 204 124 C 248 154 233 214 201 243 C 170 275 116 287 86 255 C 65 232 66 201 84 167 Z','url(#sega-panel)','#565f6d',1.2)
        a.path('M 366 114 C 417 83 466 97 505 139 C 541 175 547 220 520 251 C 495 283 443 268 400 250 C 356 232 333 205 339 168 C 342 143 349 125 366 114 Z','url(#sega-panel)','#565f6d',1.2)
        a.path('M 79 203 Q 82 144 142 120','none','#788290',.8)
        a.path('M 364 129 Q 404 98 449 117','none','#6b7687',1)
        a.sega_dpad(153,185,49)
        a.rect(279,173,40,14,'#121923',4,'#66707d',.8)
        a.rect(282,174,34,11,'url(#sega-silver)',3,'#bfc5cc',.7)
        a.text(299,200,'START','#b9c0ce',9)
        a.hit(8,'START',[279,171,40,18],'Menu buttons')
        positions=((373,170),(423,147),(472,131),(387,224),(444,206),(500,186))
        bits=(512,1024,256,2048,2,1)
    else:
        # One broad inset faceplate, unlike the Genesis pad's two glossy wells.
        a.path('M 103 134 C 170 108 239 116 300 116 C 361 116 430 108 497 134 C 533 161 549 203 540 243 C 535 270 515 303 488 300 C 436 279 369 258 300 252 C 231 258 164 279 112 300 C 85 303 65 270 60 243 C 51 203 67 161 103 134 Z','url(#sega-panel)','#626c7b',1)
        a.path('M 88 152 Q 169 110 291 121','none','#5b6777',.8)
        a.sega_dpad(145,201,48)
        a.path('M 280 230 Q 299 219 320 231 Q 301 246 280 235 Z','url(#sega-silver)','#6c7582',1)
        a.text(299,216,'START','#a6afbf',9)
        a.hit(8,'START',[279,223,42,19],'Menu buttons')
        positions=((374,193),(425,162),(477,138),(390,244),(451,220),(510,198))
        bits=(2048,1024,512,2,1,256)
    for i,(bit,label,(x,y)) in enumerate(zip(bits,('X','Y','Z','A','B','C'),positions)):
        radius=(16 if key=='saturn' else 17) if i<3 else 23
        fill='url(#sega-silver)' if key=='genesis' and i<3 else '#262d38'
        ink='#4d5665' if key=='genesis' and i<3 else '#a5aebe'
        a.face(bit,label,x,y,radius,fill,ink=ink,inside=True)
    a.finish()

for key,title,saturn,pce in (('pce','PC Engine · six-button pad',False,True),):
    a=Art(key,title,note='Extra face buttons need six-button mode in the core options.' if pce else '')
    if saturn:a.shoulders(4096,8192)
    a.path('M 139 92 Q 297 46 464 87 C 540 108 587 220 537 282 Q 506 309 468 259 L 405 226 Q 300 248 203 226 L 133 278 Q 74 316 53 263 C 22 192 63 111 139 92 Z','url(#plastic)' if pce else 'url(#black)','#75777e',3)
    a.dpad(145,175)
    a.small(8,'RUN' if pce else 'START',255,152,65)
    if not saturn:a.small(4,'SELECT' if pce else 'MODE',255,197,65)
    labels=('IV','V','VI','III','II','I') if pce else ('X','Y','Z','A','B','C')
    bits=(1024,512,256,2048,2,1) if pce else ((2048,1024,512,2,1,256) if saturn else (512,1024,256,2048,2,1))
    for i,(bit,label) in enumerate(zip(bits,labels)):
        a.button(bit,label,367+(i%3)*60,145+(i//3)*58-(i%3)*9,20 if i<3 else 24,'#777b85' if i<3 else '#35383e')
    a.text(296,118,'PC ENGINE' if pce else 'SATURN' if saturn else '16-BIT CONTROL PAD','#545861' if pce else '#bbbfc8',13)
    a.finish()

a=Art('psx','PlayStation · DualShock',note='Analog sticks require an analog-capable controller mode in the core.')
a.shoulders(4096,8192,('L2','R2'),39);a.shoulders(512,256,('L1','R1'),72)
a.path('M 150 94 H 450 C 508 93 535 143 546 207 L 569 299 Q 570 329 538 326 L 441 264 L 393 241 H 207 L 158 264 L 62 326 Q 29 329 31 299 L 54 207 C 65 143 92 93 150 94 Z','url(#plastic)','#767b83',3)
a.circle(146,164,68,'#b2b5bb','#8a8e95',2);a.circle(454,164,68,'#b2b5bb','#8a8e95',2)
a.dpad(146,164,27)
a.small(4,'SELECT',239,162,53,19);a.small(8,'START',309,162,53,19)
for bit,label,x,y,c in ((1024,'△',454,121,'#6fbdab'),(1,'○',496,164,'#e9818b'),(2,'×',454,207,'#8faff5'),(2048,'□',412,164,'#d0a4cd')):
    a.button(bit,label,x,y,20,ink=c)
a.stick(232,239,click=(16384,'L3'));a.stick(369,239,(2,3),click=(32768,'R3'))
a.text(300,126,'DUALSHOCK','#626771',13);a.finish()

a=Art('n64','Nintendo 64 controller',note='Z is on the rear of the controller, behind the control stick.')
a.shoulders()
a.path('M 135 85 Q 300 46 465 85 C 524 97 551 125 550 195 L 535 298 Q 518 334 492 300 L 447 225 L 365 199 L 333 310 Q 300 356 266 310 L 235 199 L 153 225 L 108 300 Q 82 334 65 298 L 50 195 C 49 125 76 97 135 85 Z','url(#plastic)','#7c818a',3)
a.dpad(139,164,24)
a.button(8,'START',300,142,17,'#cb3e4c')
a.stick(300,234,r=32)
a.circle(300,234,33,'#b2b5bc','#7e838d',2)
a.path('M 283 225 L 283 214 L 295 207 L 307 208 L 318 219 L 318 230 L 306 238 L 292 237 Z','#d4d6d8','#777d87',2)
a.circle(300,222,9,'#bbc0c4','#91979e',1)
a.button(2048,'B',387,159,21,'#259d80');a.button(2,'A',416,208,23,'#376eb4')
for action,label,x,y in (('axis:3:-1','▲',476,126),('axis:2:-1','◀',453,151),('axis:2:1','▶',501,151),('axis:3:1','▼',476,176)):
    a.button(action,label,x,y,13,'#e1b733','C buttons',ink='#48390e')
a.small(4096,'Z · REAR',255,24,90,23,'Shoulders & triggers');a.finish()

for key,title in (('gb','Game Boy'),('gbc','Game Boy Color')):
    a=Art(key,title,(300,440))
    a.path('M 51 20 H 249 Q 266 20 266 38 V 353 Q 266 416 215 421 H 51 Q 32 421 32 402 V 38 Q 32 20 51 20 Z','url(#plastic)' if key=='gb' else 'url(#purple)','#7f8289',3)
    a.screen(74,64,150,129,key=='gb')
    a.text(142,224,'GAME BOY' if key=='gb' else 'GAME BOY COLOR','#3c4375' if key=='gb' else '#dedde8',15)
    a.dpad(87,282,21)
    a.button(2,'B',181,292,19,'#8b2756');a.button(1,'A',228,270,19,'#8b2756')
    a.small(4,'SELECT',92,347,51,18);a.small(8,'START',153,347,49,18)
    for x in range(184,235,9): a.path(f'M {x} 377 l 14 17','none','#747681',3)
    a.finish()

for key,title in (('gba','Game Boy Advance'),('gg','Game Gear'),('lynx','Atari Lynx'),('ngp','Neo Geo Pocket'),('ngpc','Neo Geo Pocket Color'),('psp','PlayStation Portable')):
    a=Art(key,title)
    if key in ('gba','psp'):a.shoulders()
    fill='url(#purple)' if key=='gba' else ('#93a9b8' if key=='ngpc' else 'url(#plastic)' if key=='ngp' else 'url(#black)')
    a.path('M 109 84 Q 300 61 491 84 C 551 90 573 129 568 190 C 564 256 532 281 480 278 Q 300 303 120 278 C 68 281 36 256 32 190 C 27 129 49 90 109 84 Z',fill,'#6c707a',3)
    if key=='psp':a.screen(191,118,218,123)
    elif key in ('gg','ngp','ngpc'):a.screen(223,111,154,146,key=='ngp')
    else:a.screen(191,111,218,146)
    if key in ('ngp','ngpc'):a.stick(111,167,r=34,digital=True)
    else:a.dpad(111,167,23 if key!='psp' else 21)
    if key=='psp':
        for bit,label,x,y,c in ((1024,'△',489,129,'#79c4ae'),(1,'○',526,166,'#e78b98'),(2,'×',489,203,'#93b3f0'),(2048,'□',452,166,'#c8a6d5')):a.button(bit,label,x,y,17,ink=c)
        a.stick(112,242,r=22)
        a.small(4,'SELECT',432,252,50,16);a.small(8,'START',492,252,50,16)
    else:
        a.button(2,'1' if key=='gg' else 'B',459,197,22,'#4e5157');a.button(1,'2' if key=='gg' else 'A',512,164,22,'#4e5157')
        if key=='gba':
            a.small(8,'START',73,235,57,17);a.small(4,'SELECT',73,259,57,17)
        elif key=='lynx':
            for bit,label,x in ((512,'OPT 1',427),(8,'PAUSE',475),(256,'OPT 2',523)):a.small(bit,label,x-21,244,42,17)
        else:a.small(8,'OPTION' if key in ('ngp','ngpc') else 'START',460,107,69,18)
    a.text(300,281,title.upper(),'#cecfda' if key not in ('ngp','ngpc') else '#394450',12)
    a.finish()

a=Art('nds','Nintendo DS', (450,460), 'Use the mouse or touchscreen for the lower display.')
a.rect(65,18,320,193,'url(#plastic)',20,'#7d8188',3);a.screen(131,38,188,144)
a.rect(54,218,342,219,'url(#plastic)',20,'#7d8188',3)
a.rect(69,209,312,15,'#8e929a',7);a.screen(148,255,154,133)
a.dpad(101,309,18)
for bit,label,x,y in ((1024,'X',351,282),(1,'A',374,307),(2,'B',351,332),(2048,'Y',328,307)):a.button(bit,label,x,y,11,'#535760')
a.small(8,'START',325,364,56,16);a.small(4,'SELECT',325,389,56,16)
a.small(512,'L',62,220,58,20,'Shoulders & triggers');a.small(256,'R',330,220,58,20,'Shoulders & triggers')
a.finish()

for key,title in (('sms','Master System control pad'),('sg1000','SG-1000 controller')):
    a=Art(key,title)
    a.rect(61,104,478,175,'url(#black)' if key=='sms' else 'url(#plastic)',12,'#73777f',3)
    a.rect(79,121,146,138,'#4b4f57',5,'#171b20',2)
    if key=='sg1000':a.stick(152,191,r=35,digital=True)
    else:a.dpad(152,191,26)
    a.button(2,'1',363,208,26,'#343940');a.button(1,'2',457,208,26,'#343940')
    a.text(411,153,'CONTROL PAD','#8c9bb2',16);a.finish()

for key,title in (('atari2600','Atari 2600 · CX40 joystick'),('odyssey2','Odyssey² joystick'),('atari7800','Atari 7800 · ProLine joystick')):
    a=Art(key,title)
    if key=='atari7800':
        a.rect(189,75,222,247,'url(#black)',35,'#6c7077',3)
        a.small(2,'1',175,168,32,85,'Buttons');a.small(1,'2',393,168,32,85,'Buttons')
        a.rect(208,258,184,27,'#babdc1',3);a.text(300,278,'PROLINE','#282a2d',14)
    else:
        a.path('M 128 125 H 472 L 512 298 H 88 Z','url(#black)','#71757c',3)
        a.rect(132,136,336,130,'#24272c',12,'#575b64',2)
        a.button(2,'FIRE',171,170,24,'#c23d40',ink='#fff')
    # A raised shaft and concentric boot distinguish a joystick from a pad.
    for r in (61,49,37):a.circle(300,208,r,'url(#rubber)','#74777d',2)
    a.rect(288,100,24,106,'url(#black)',8,'#777',2)
    a.circle(300,104,34,'url(#rubber)','#848890',2)
    for bit,label,x,y in ((64,'↑',300,76),(128,'↓',300,229),(32,'←',254,179),(16,'→',346,179)):
        a.hit(bit,label,[x-18,y-18,36,36],'Joystick')
    a.finish()

for key,title in (('colecovision','ColecoVision hand controller'),('intellivision','Intellivision hand controller'),('atari5200','Atari 5200 controller')):
    a=Art(key,title,(300,480),note='Keypad functions follow the selected core’s controller mode.')
    a.rect(64,20,172,434,'#7b6044' if key=='intellivision' else 'url(#black)',20,'#969089',3)
    a.rect(77,38,146,396,'#d1bb8b' if key=='intellivision' else '#34373b',12,'#16191d',2)
    y0=67 if key=='intellivision' else 228
    keypad_bits = ([2048,1024,512,256,4096,8192,16384,32768,'axis:1:1',8,'axis:0:1',4] if key=='colecovision'
                   else [8192,None,4096,None,32768,None,16384,None,None,2048,256,1024] if key=='atari5200'
                   else [None,None,None,None,32768,None,None,None,None,4096,16384,8192])
    for i,label in enumerate(('1','2','3','4','5','6','7','8','9','*','0','#')):
        x,y=91+i%3*41,y0+i//3*40
        a.small(keypad_bits[i],label,x,y,36,31,'Keypad',fill='#3c3b38')
    a.stick(150,338 if key=='intellivision' else 136,r=44,digital=key=='colecovision',brass=key=='intellivision')
    if key=='atari5200':
        for x in (49,227):
            a.small(2,'2',x,103,24,43,'Buttons');a.small(1,'1',x,153,24,43,'Buttons')
    else:
        a.small(2,'1',49,130,24,68,'Buttons');a.small(1,'2',227,130,24,68,'Buttons')
    if key=='intellivision':
        a.small(2048,'TOP',49,79,24,40,'Buttons');a.small(2048,'TOP',227,79,24,40,'Buttons')
    if key=='atari5200':
        a.small(8,'START',79,50,43,20);a.small(4,'PAUSE',129,50,43,20);a.small(None,'RESET',179,50,43,20)
    a.finish()

a=Art('vectrex','Vectrex controller')
a.rect(52,110,496,188,'url(#black)',12,'#797d86',3)
a.rect(70,127,460,151,'#252c34',7,'#7f899a',2)
a.stick(166,200,r=37,digital=True)
for bit,label,x in ((1,'1',309),(2,'2',369),(1024,'3',429),(2048,'4',489)):a.button(bit,label,x,213,20,'#dedfdd',ink='#242832')
a.text(405,160,'VECTREX','#bfcada',18);a.finish()

a=Art('vb','Virtual Boy controller')
a.shoulders()
a.path('M 137 100 Q 300 48 463 100 C 523 100 555 144 545 212 L 515 308 Q 488 334 463 292 L 409 238 Q 300 267 191 238 L 137 292 Q 112 334 85 308 L 55 212 C 45 144 77 100 137 100 Z','url(#black)','#7a7b83',3)
a.dpad(139,177,26,group='Left directional pad')
a.dpad(458,177,26,(4096,16384,8192,32768),group='Right directional pad')
a.button(2,'B',336,214,20,'#c33843');a.button(1,'A',384,184,20,'#c33843')
a.small(4,'SELECT',213,201,56);a.small(8,'START',213,239,56);a.finish()

for key,title,color in (('ws','WonderSwan','#babfc6'),('wsc','WonderSwan Color','#4177a1')):
    a=Art(key,title)
    a.rect(42,65,516,255,color,27,'#777f8a',3)
    a.screen(223,101,235,167,key=='ws')
    # X and Y are four-button clusters, not analog sticks.
    a.dpad(126,142,21,(8192,4096,512,256),('Y1','Y3','Y4','Y2'),'Y buttons')
    a.dpad(126,252,21,(64,128,32,16),('X1','X3','X4','X2'),'X buttons')
    a.button(2,'B',490,247,18,'#434951');a.button(1,'A',532,222,18,'#434951')
    a.small(8,'START',228,292,62,17);a.text(352,301,title.upper(),'#303d50' if key=='ws' else '#edf0f4',13)
    a.finish()

(DEST/'layouts.json').write_text(json.dumps(CATALOG,indent=2)+'\n')
