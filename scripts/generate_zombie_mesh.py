"""Generate the original Zombiebox shambler GLB, previews, and Rust mesh data.

Requires numpy, trimesh, and Pillow. Coordinates: Y up, +Z forward, feet at Y=0.
Run from anywhere: `python3 scripts/generate_zombie_mesh.py`.

Artifacts (GLB and preview PNGs) are written to `target/zombie-mesh/`; the
committed Rust mesh is written to
`crates/zombiebox-render3d/src/mesh/zombie.rs`.
"""
import numpy as np
import trimesh
from PIL import Image, ImageDraw, ImageFont
from pathlib import Path
from collections import defaultdict
import math
import shutil
import subprocess

REPO_ROOT = Path(__file__).resolve().parent.parent
ARTIFACT_DIR = REPO_ROOT / "target" / "zombie-mesh"
RUST_OUT = REPO_ROOT / "crates" / "zombiebox-render3d" / "src" / "mesh" / "zombie.rs"
ARTIFACT_DIR.mkdir(parents=True, exist_ok=True)
parts=defaultdict(list)
# sRGB palette, intentionally desaturated
bone=(176,174,139); bone_hi=(203,199,161); bone_shadow=(111,116,91)
skin=(111,126,91); skin_dark=(77,91,68); socket=(33,36,28); mouth=(41,31,27)
coat=(74,83,65); coat_light=(99,107,83); coat_dark=(48,56,47); seam=(128,132,102)
shirt=(133,121,92); shirt_dark=(87,78,60); pants=(65,77,79); pants_hi=(88,99,96); pants_dark=(43,53,57)
boot=(48,39,33); boot_hi=(72,57,43); sole=(29,27,24); metal=(146,139,104); stain=(80,48,37)

def tri(part,a,b,c,col,desired=None):
    a,b,c=map(lambda x:np.array(x,float),(a,b,c)); n=np.cross(b-a,c-a)
    if np.linalg.norm(n)<1e-10:return
    if desired is not None and np.dot(n,desired)<0: b,c=c,b
    parts[part].append((np.array([a,b,c]),tuple(col)))
def poly(part,pts,col,normal=(0,0,1)):
    for i in range(1,len(pts)-1):tri(part,pts[0],pts[i],pts[i+1],col,normal)
def shade(col,k): return tuple(int(np.clip(c*k,0,255)) for c in col)
# beveled octagonal horizontal ring, with broad front face at z=+depth
cross=np.array([[-.60,1],[.60,1],[1,.60],[1,-.60],[.60,-1],[-.60,-1],[-1,-.60],[-1,.60]])
def rings(part, specs, col, colors=None):
    # spec y, center x, center z, half width, half depth
    rows=[np.array([[cx+x*w,y,cz+z*d] for x,z in cross]) for y,cx,cz,w,d in specs]
    for j in range(len(rows)-1):
        for i in range(8):
            n=(i+1)%8; a,b=rows[j][i],rows[j][n]; c,d=rows[j+1][n],rows[j+1][i]
            center=(a+b+c+d)/4; axis=np.array([(specs[j][1]+specs[j+1][1])/2,center[1],(specs[j][2]+specs[j+1][2])/2]); outward=center-axis
            cc=colors[j][i] if colors else shade(col,[1,.95,.86,.82,.82,.86,.93,.96][i]);
            tri(part,a,b,c,cc,outward);tri(part,a,c,d,cc,outward)
    poly(part,rows[0],shade(col,.72),(0,-1,0));poly(part,rows[-1],shade(col,1.08),(0,1,0))
def box(part,center,size,col):
    m=trimesh.creation.box(extents=size); m.apply_translation(center)
    for face in m.faces:tri(part,*m.vertices[face],col)
def ell(part,center,scale,col,sub=1):
    m=trimesh.creation.icosphere(subdivisions=sub); m.vertices*=scale;m.vertices+=center
    for i,f in enumerate(m.faces):tri(part,*m.vertices[f],shade(col,[.96,1,1.04,1][i%4]))
def beam(part,a,b,r1,r2,col,n=6):
    a,b=np.array(a,float),np.array(b,float); v=b-a; v/=np.linalg.norm(v); u=np.cross(v,[0,0,1]);
    if np.linalg.norm(u)<.01:u=np.cross(v,[1,0,0])
    u/=np.linalg.norm(u); w=np.cross(v,u); p=[a+r1*(u*np.cos(t)+w*np.sin(t)) for t in np.arange(n)*2*np.pi/n]; q=[b+r2*(u*np.cos(t)+w*np.sin(t)) for t in np.arange(n)*2*np.pi/n]
    poly(part,p,col,-v);poly(part,q,col,v)
    for i in range(n):
        j=(i+1)%n; desired=(p[i]+p[j])/2-a;tri(part,p[i],p[j],q[j],col,desired);tri(part,p[i],q[j],q[i],col,desired)
# Body: narrow waist, broad hunched shoulders, exposed dirty shirt between jacket panels.
p='Body'
rings(p,[(.405,0,0,.111,.073),(.475,0,-.004,.125,.084),(.60,0,-.012,.147,.10),(.735,0,-.012,.169,.103),(.778,0,-.006,.125,.079)],coat)
# neck
rings(p,[(.754,0,.013,.046,.044),(.831,0,.024,.046,.041)],skin_dark)
# front surface from waist to upper chest, deliberately staggered torn jacket edges.
poly(p,[(-.038,.425,.086),(.046,.425,.086),(.061,.60,.093),(.060,.72,.094),(.017,.767,.083),(-.054,.719,.094),(-.066,.60,.093)],shirt)
# left and right lapels, ragged coat hems
poly(p,[(-.095,.736,.095),(-.037,.771,.084),(-.012,.716,.106),(-.050,.659,.104),(-.037,.614,.096),(-.092,.676,.103)],coat_light)
poly(p,[(.090,.736,.095),(.037,.770,.084),(.012,.709,.106),(.052,.654,.104),(.043,.600,.096),(.105,.681,.103)],coat_light)
poly(p,[(-.111,.49,.085),(-.038,.48,.093),(-.052,.418,.088),(-.075,.439,.085),(-.092,.393,.076),(-.116,.42,.075)],coat_dark)
poly(p,[(.037,.482,.093),(.119,.49,.085),(.114,.42,.080),(.089,.409,.080),(.077,.385,.073),(.061,.43,.085),(.04,.414,.083)],coat_light)
# pockets, torn fabric, seams and subtle rust stains
poly(p,[(-.143,.627,.087),(-.080,.631,.103),(-.078,.581,.102),(-.13,.576,.089)],coat_dark)
poly(p,[(-.141,.631,.09),(-.079,.635,.106),(-.078,.617,.106),(-.14,.613,.09)],coat_light)
poly(p,[(.084,.576,.104),(.137,.58,.089),(.118,.546,.09),(.103,.553,.104),(.091,.530,.101)],stain)
poly(p,[(-.021,.61,.099),(.038,.622,.099),(.024,.586,.099),(.04,.575,.099),(-.025,.566,.099)],shirt_dark)
# ribs beneath torn shirt opening, understated
for y in [.602,.586,.57]:
    beam(p,(-.022,y,.101),(.024,y-.004,.101),.0038,.0033,bone_shadow,5)
# belt
rings(p,[(.407,0,-.001,.117,.081),(.428,0,-.001,.117,.081)],boot)
box(p,(.005,.418,.084),(.041,.023,.012),metal);box(p,(.005,.418,.092),(.025,.011,.005),boot)
# back seams and fabric tears, geometry visible from overhead
poly(p,[(-.01,.756,-.111),(.005,.759,-.111),(.011,.568,-.116),(-.004,.49,-.096)],coat_dark,(0,0,-1))
poly(p,[(-.104,.695,-.113),(-.043,.699,-.116),(-.074,.658,-.117),(-.038,.647,-.118),(-.087,.618,-.118)],coat_light,(0,0,-1))
# Head: broad forehead, temples, pinched chin; forward +Z.
p='Head'; hx=0; hz=.025
rings(p,[(.814,0,.030,.046,.046),(.835,0,.031,.063,.061),(.872,0,.025,.084,.071),(.921,0,.019,.094,.080),(.961,0,.010,.089,.077),(.993,0,.003,.063,.059),(1.004,0,.001,.034,.035)],bone)
# scalp patches wrapping the back / sides
poly(p,[(-.038,.992,-.057),(.037,.989,-.059),(.070,.959,-.068),(.048,.923,-.064),(.001,.937,-.069),(-.046,.918,-.066),(-.074,.954,-.064)],skin_dark,(0,0,-1))
# eye sockets: irregular polygons with bony rim, dark central recess, angular brow
for s in [-1,1]:
    # use mirror on coordinates, and front-facing outward normal
    outer=[(.012,.935,.105),(.055,.947,.103),(.081,.929,.088),(.070,.902,.095),(.037,.892,.102),(.014,.909,.105)]
    inner=[(.022,.928,.107),(.053,.936,.106),(.069,.926,.098),(.060,.910,.104),(.037,.903,.110),(.024,.913,.109)]
    outer=[(s*x,y,z) for x,y,z in outer]; inner=[(s*x,y,z) for x,y,z in inner]
    for i in range(6):
        j=(i+1)%6;poly(p,[outer[i],outer[j],inner[j],inner[i]],bone_shadow if i>=2 else bone_hi)
    poly(p,inner,socket)
    # small dull, dead eye glint rather than glowing cartoon eyes
    ell(p,(s*.047,.919,.109),(.006,.006,.004),(143,141,93),0)
    # pronounced cheekbones
    poly(p,[(s*.030,.891,.109),(s*.076,.903,.096),(s*.082,.878,.087),(s*.051,.871,.103)],bone_hi)
# nose hollow, black triangle with split base
poly(p,[(-.010,.919,.108),(.012,.919,.108),(.022,.881,.106),(.003,.886,.112),(-.017,.879,.108)],socket)
# mouth dark opening; separated rows of irregular teeth
poly(p,[(-.050,.868,.105),(-.030,.875,.108),(.029,.875,.108),(.052,.862,.102),(.047,.838,.096),(.023,.828,.091),(-.025,.829,.091),(-.048,.843,.099)],mouth)
for row,y in enumerate([.865,.838]):
    for i in range(7):
        x=(i-3)*.012; z=.108-abs(x)*.11-(.006 if row else 0)
        if row==1 and i==5:continue
        box(p,(x,y+(i%3-1)*.0015,z),(.0095,.014 if row==0 else .010,.009),bone_hi if i%2 else bone)
# angular jawline and crack in forehead
poly(p,[(-.055,.845,.099),(-.032,.824,.094),(.031,.824,.094),(.054,.845,.099),(.041,.818,.078),(-.039,.818,.078)],bone_shadow)
poly(p,[(-.02,.985,.066),(-.012,.966,.090),(-.024,.957,.096),(-.014,.947,.104),(-.029,.959,.098),(-.020,.967,.091)],skin_dark)
# ears remnants
for s in [-1,1]:ell(p,(s*.094,.899,.013),(.012,.024,.015),skin_dark,0)
# legs & boots: mesh includes both trouser and boot; each retains hip articulation.
for s,part in [(-1,'LegL'),(1,'LegR')]:
    x=s*.0912
    rings(part,[(.14,x,.003,.050,.052),(.205,x,0,.056,.062),(.30,x,-.006,.062,.064),(.429,x,0,.067,.073)],pants)
    # ragged pant ends and knee tears
    z=.067
    poly(part,[(x-.046,.25,z),(x+.035,.265,z),(x+.043,.229,z),(x+.018,.237,z),(x+.005,.213,z),(x-.038,.224,z)],pants_dark)
    poly(part,[(x-.026,.249,z+.003),(x+.018,.251,z+.003),(x+.027,.235,z+.003),(x-.008,.225,z+.003),(x-.024,.234,z+.003)],skin if s<0 else pants_hi)
    poly(part,[(x-.044,.18,.057),(x-.015,.177,.060),(x-.024,.132,.054),(x-.042,.144,.055)],pants_hi)
    # boot soles are wide and protrude in front, chamfered toe and leather shaft
    rings(part,[(0,x,.034,.066,.104),(.020,x,.034,.069,.107),(.032,x,.034,.067,.104)],sole)
    rings(part,[(.030,x,.033,.064,.101),(.067,x,.028,.064,.095),(.092,x,.014,.055,.075),(.115,x,.002,.051,.059)],boot_hi)
    rings(part,[(.075,x,.001,.052,.061),(.147,x,0,.054,.057),(.169,x,0,.057,.058)],boot)
    rings(part,[(.151,x,0,.059,.061),(.163,x,0,.059,.061)],boot_hi)
    # boot tongue, eyelets, crossing laces
    poly(part,[(x-.024,.075,.086),(x+.024,.075,.086),(x+.023,.15,.063),(x-.023,.15,.063)],boot_hi)
    for i in range(3):
        y=.092+i*.018; z=.082-i*.007
        beam(part,(x-.019,y,z),(x+.019,y+.011,z-.004),.0028,.0028,metal,4)
        beam(part,(x+.019,y,z),(x-.019,y+.011,z-.004),.0025,.0025,metal,4)
    # tread strips
    for k in [-1,0,1]:box(part,(x,.016,.033+k*.055),(.145,.011,.014),boot)
# arms: tattered sleeves, exposed green-gray forearms, knuckles, crooked fingers
for s,part in [(-1,'ArmL'),(1,'ArmR')]:
    x=s*.228; # actual existing animation shoulder location
    rings(part,[(.594 if s<0 else .555,x,0,.041,.047),(.65,x,0,.045,.048),(.716,x-s*.012,0,.060,.054),(.744,x-s*.035,0,.064,.056),(.759,x-s*.048,0,.046,.045)],coat)
    # forearm overlaps sleeve with jagged cuffs
    rings(part,[(.488,x+s*.003,.009,.031,.032),(.545,x,.005,.038,.039),(.627,x,0,.040,.043)],skin)
    y=.594 if s<0 else .555
    poly(part,[(x-.042,y+.02,.049),(x+.041,y+.02,.049),(x+.040,y-.015,.046),(x+.018,y-.003,.046),(x+.002,y-.026,.046),(x-.013,y-.007,.046),(x-.039,y-.017,.046)],coat_dark)
    poly(part,[(x-.037,.69,.053),(x+.018,.696,.053),(x+.032,.659,.05),(x+.008,.671,.053),(x-.010,.65,.05)],coat_light)
    # wrist, hand, claw fingers with individual segments
    ell(part,(x+s*.003,.472,.012),(.036,.043,.027),skin_dark,1)
    for i in range(4):
        xx=x+(i-1.5)*.015; yy=.442-([.006,.013,.012,0][i]);
        beam(part,(xx,.463,.022),(xx+s*.003,yy,.031),.0085,.0065,skin,5)
        beam(part,(xx+s*.003,yy,.031),(xx+s*.004,yy+.002,.041),.0065,.004,bone_shadow,5)
    beam(part,(x-s*.025,.484,.021),(x-s*.044,.463,.031),.012,.007,skin,5)
# GLB scene: six separate mesh nodes, local origins at animation pivots.
origins={'Body':(0,.588,0),'Head':(0,.882,0),'ArmL':(-.228,.735,0),'ArmR':(.228,.735,0),'LegL':(-.0912,.4214,0),'LegR':(.0912,.4214,0)}
# exact instance centers and full scale of current animation.rs for Shambler
h=.98;r=.24
centers={'Body':(0,h*.60,0),'Head':(0,h*.90,0),'ArmL':(-r*.95,h*.60,0),'ArmR':(r*.95,h*.60,0),'LegL':(-r*.38,h*.21,0),'LegR':(r*.38,h*.21,0)}
scales={'Body':(r*1.45,h*.38,r*.95),'Head':(r*.85,h*.20,r*.8),'ArmL':(r*.40,h*.30,r*.45),'ArmR':(r*.40,h*.30,r*.45),'LegL':(r*.55,h*.44,r*.65),'LegR':(r*.55,h*.44,r*.65)}
scene=trimesh.Scene()
for p,faces in parts.items():
    v=np.concatenate([a for a,c in faces])-origins[p]; f=np.arange(len(v)).reshape(-1,3); color=np.repeat([(*c,255) for a,c in faces],3,axis=0)
    mesh=trimesh.Trimesh(vertices=v,faces=f,vertex_colors=color,process=False)
    tr=np.eye(4);tr[:3,3]=origins[p];scene.add_geometry(mesh,node_name='Zombie'+p,geom_name='Zombie'+p,transform=tr)
scene.export(ARTIFACT_DIR/'shambler.glb')
# Native mesh data in Rust, normalized to existing draw_actor part transforms; preserves flat normals.
# palette per face, generated without external runtime dependencies.
with RUST_OUT.open('w') as f:
    f.write('//! Generated original Zombiebox shambler meshes. Y up, +Z forward.\n//! Part coordinates are normalized for the rigid-part transforms in `animation.rs`.\nuse super::{GeometryTag, MaterialFlags, MeshBuffer, MeshBuilder, VertexMeta, WorldVertex};\nuse glam::Vec3;\n\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub enum ZombieMeshPart { Body, Head, ArmL, ArmR, LegL, LegR }\n\npub fn zombie_mesh(part: ZombieMeshPart) -> MeshBuffer {\n    let triangles = match part {\n')
    for p in parts:f.write(f'        ZombieMeshPart::{p} => {p.upper()},\n')
    f.write('    };\n    let meta = VertexMeta::DEFAULT.with_tag(GeometryTag::Actor).with_flags(MaterialFlags::ACTOR_MIN_LIGHT | MaterialFlags::CUTAWAY_EXEMPT);\n    let mut b = MeshBuilder::new();\n    for &(points, color) in triangles {\n        let [a, c, d] = points.map(Vec3::from_array);\n        b.push_triangle(a, c, d, WorldVertex::rgba8(color[0], color[1], color[2], 255), meta);\n    }\n    b.build()\n}\n\ntype Triangle = ([[f32; 3]; 3], [u8; 3]);\n')
    for p,faces in parts.items():
        f.write(f'const {p.upper()}: &[Triangle] = &[\n')
        for a,c in faces:
            v=(a-np.array(centers[p]))/np.array(scales[p]); pts=', '.join('['+', '.join(f'{x:.6f}' for x in row)+']' for row in v); f.write(f'    ([{pts}], [{c[0]}, {c[1]}, {c[2]}]),\n')
        f.write('];\n')

if shutil.which('rustfmt'):
    subprocess.run(['rustfmt', '--edition', '2024', str(RUST_OUT)], check=True)

# Render a studio preview using orthographic triangle rasterization and z-buffer.
def render(az,el,focus=(0,.51,0),height=1.18,W=720,H=920):
    az,el=np.radians([az,el]); toward=np.array([np.sin(az)*np.cos(el),np.sin(el),np.cos(az)*np.cos(el)]); right=np.cross([0,1,0],toward); right/=np.linalg.norm(right); up=np.cross(toward,right)
    yy,xx=np.mgrid[0:H,0:W]; bg=np.clip(1-(((xx-W*.5)/(W*.85))**2+((yy-H*.46)/(H*.85))**2),0,1); img=np.stack([19+25*bg,23+27*bg,23+25*bg],axis=-1).astype(np.uint8); depth=np.full((H,W),-np.inf)
    # studio contact shadow on backdrop
    x0=W*.5; y0=H*.5+np.dot(np.array(focus),up)*H/height; sh=np.exp(-((xx-x0)/(W*.24))**2-((yy-y0)/(H*.024))**2)*.56;img=(img*(1-sh[...,None])).astype(np.uint8)
    key=np.array([-.5,.8,.9]);key/=np.linalg.norm(key); fill=np.array([.9,.3,-.5]);fill/=np.linalg.norm(fill)
    for faces in parts.values():
      for a,col in faces:
        n=np.cross(a[1]-a[0],a[2]-a[0]);n/=np.linalg.norm(n)
        if np.dot(n,toward)<=0:continue
        v=a-np.array(focus); q=np.stack([v@right*H/height+W/2,-v@up*H/height+H/2,v@toward],axis=1)
        x1=max(0,int(np.floor(q[:,0].min())));x2=min(W,int(np.ceil(q[:,0].max()))+1);y1=max(0,int(np.floor(q[:,1].min())));y2=min(H,int(np.ceil(q[:,1].max()))+1)
        if x1>=x2 or y1>=y2:continue
        X,Y=np.meshgrid(np.arange(x1,x2)+.5,np.arange(y1,y2)+.5); A,B,C=q; den=(B[1]-C[1])*(A[0]-C[0])+(C[0]-B[0])*(A[1]-C[1]);
        if abs(den)<1e-9:continue
        u=((B[1]-C[1])*(X-C[0])+(C[0]-B[0])*(Y-C[1]))/den;v=((C[1]-A[1])*(X-C[0])+(A[0]-C[0])*(Y-C[1]))/den;w=1-u-v; z=u*A[2]+v*B[2]+w*C[2]; d=depth[y1:y2,x1:x2];mask=(u>=-1e-7)&(v>=-1e-7)&(w>=-1e-7)&(z>d)
        light=.64+.43*max(0,n@key)+.17*max(0,n@fill); c=np.clip(np.array(col)*light,0,255).astype(np.uint8);img[y1:y2,x1:x2][mask]=c;d[mask]=z[mask]
    return Image.fromarray(img)
# main turntable contact sheet with close-up and rear
canvas=Image.new('RGB',(1560,1040),(23,27,27)); d=ImageDraw.Draw(canvas)
font='/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf'; bold='/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf'
d.text((42,25),'ZOMBIEBOX  /  SHAMBLER',font=ImageFont.truetype(bold,36),fill=(216,220,196));d.text((43,70),'LOW-POLY CHARACTER  •  VERTEX COLOR  •  SIX ARTICULATED PARTS',font=ImageFont.truetype(font,18),fill=(149,164,145))
for x,az,el,label in [(24,-25,9,'01  /  THREE-QUARTER'),(532,0,2,'02  /  FRONT'),(1040,155,10,'03  /  REAR')]:
    im=render(az,el,W=496,H=866);canvas.paste(im,(x,116));d.text((x+16,992),label,font=ImageFont.truetype(bold,16),fill=(171,184,165))
canvas.save(ARTIFACT_DIR/'shambler_preview.png')
render(-13,4,focus=(0,.90,.01),height=.29,W=660,H=660).save(ARTIFACT_DIR/'shambler_face.png')
print('Triangles:',{p:len(v) for p,v in parts.items()},'total',sum(map(len,parts.values())))
# Validate GLB roundtrip
loaded=trimesh.load(ARTIFACT_DIR/'shambler.glb',force='scene');print('GLB geometries',list(loaded.geometry),'bounds',loaded.bounds)
