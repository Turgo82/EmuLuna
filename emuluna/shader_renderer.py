"""OpenGL preset pipeline. Frames stay on the GPU through final presentation.

Only ROM-sized input pixels are uploaded. No readback, image flipping, CPU
scaling, or per-frame framebuffer allocation is used during gameplay.
"""
import ctypes as C
import re
from PySide6.QtGui import QImage
from shiboken6 import VoidPtr
from .shaders import PRESETS, ShaderError, preset, parameter_values, stage_source, is_software_renderer

U, I, F, P = C.c_uint, C.c_int, C.c_float, C.c_void_p
GL_TEXTURE_2D=0x0DE1
MVP=(F*16)(2,0,0,0, 0,2,0,0, 0,0,1,0, -1,-1,0,1)


class GL:
    def __init__(self, get_proc):
        self.get_proc=get_proc

    def bind(self,name,result,*args):
        address=self.get_proc(name.encode())
        if not address:
            raise ShaderError('Graphics driver is missing '+name)
        fn=getattr(C,'WINFUNCTYPE',C.CFUNCTYPE)(result,*args)(int(address))
        setattr(self,name[2:],fn)

    def load(self):
        entries={
            'GetString':(C.c_char_p,U), 'GetIntegerv':(None,U,C.POINTER(I)),
            'Viewport':(None,I,I,I,I), 'ClearColor':(None,F,F,F,F), 'Clear':(None,U),
            'Enable':(None,U), 'Disable':(None,U), 'ColorMask':(None,U,U,U,U),
            'GenTextures':(None,I,C.POINTER(U)), 'DeleteTextures':(None,I,C.POINTER(U)),
            'BindTexture':(None,U,U), 'ActiveTexture':(None,U), 'PixelStorei':(None,U,I),
            'TexImage2D':(None,U,I,I,I,I,I,U,U,P), 'TexSubImage2D':(None,U,I,I,I,I,I,U,U,P),
            'GenerateMipmap':(None,U), 'TexParameteri':(None,U,U,I),
            'GenSamplers':(None,I,C.POINTER(U)), 'DeleteSamplers':(None,I,C.POINTER(U)),
            'BindSampler':(None,U,U), 'SamplerParameteri':(None,U,U,I),
            'GenFramebuffers':(None,I,C.POINTER(U)), 'DeleteFramebuffers':(None,I,C.POINTER(U)),
            'BindFramebuffer':(None,U,U), 'FramebufferTexture2D':(None,U,U,U,U,I),
            'CheckFramebufferStatus':(U,U),
            'GenVertexArrays':(None,I,C.POINTER(U)), 'BindVertexArray':(None,U),
            'DeleteVertexArrays':(None,I,C.POINTER(U)),
            'GenBuffers':(None,I,C.POINTER(U)), 'BindBuffer':(None,U,U),
            'BufferData':(None,U,C.c_ssize_t,P,U), 'DeleteBuffers':(None,I,C.POINTER(U)),
            'EnableVertexAttribArray':(None,U), 'DisableVertexAttribArray':(None,U),
            'VertexAttribPointer':(None,U,I,U,U,I,P), 'VertexAttrib4f':(None,U,F,F,F,F),
            'CreateShader':(U,U), 'ShaderSource':(None,U,I,C.POINTER(C.c_char_p),C.POINTER(I)),
            'CompileShader':(None,U), 'GetShaderiv':(None,U,U,C.POINTER(I)),
            'GetShaderInfoLog':(None,U,I,C.POINTER(I),P), 'DeleteShader':(None,U),
            'CreateProgram':(U,), 'AttachShader':(None,U,U), 'LinkProgram':(None,U),
            'BindAttribLocation':(None,U,U,C.c_char_p),
            'GetProgramiv':(None,U,U,C.POINTER(I)), 'GetProgramInfoLog':(None,U,I,C.POINTER(I),P),
            'UseProgram':(None,U), 'DeleteProgram':(None,U),
            'GetActiveUniform':(None,U,U,I,C.POINTER(I),C.POINTER(I),C.POINTER(U),P),
            'GetUniformLocation':(I,U,C.c_char_p),
            'Uniform1i':(None,I,I), 'Uniform1ui':(None,I,U), 'Uniform1f':(None,I,F),
            'Uniform2f':(None,I,F,F), 'Uniform4f':(None,I,F,F,F,F),
            'UniformMatrix4fv':(None,I,I,U,C.POINTER(F)), 'DrawArrays':(None,U,I,I),
        }
        for name,signature in entries.items(): self.bind('gl'+name,*signature)


class Texture:
    def __init__(self,gl,size,internal=0x8058,framebuffer=False):
        self.gl,self.size,self.internal=gl,tuple(size),internal
        self.id=U(); self.fbo=U(); self.mip_dirty=True
        gl.GenTextures(1,C.byref(self.id));gl.BindTexture(GL_TEXTURE_2D,self.id)
        gl.TexImage2D(GL_TEXTURE_2D,0,internal,*self.size,0,0x1908,0x1401,None)
        gl.TexParameteri(GL_TEXTURE_2D,0x2801,0x2600)
        gl.TexParameteri(GL_TEXTURE_2D,0x2800,0x2600)
        if framebuffer:
            gl.GenFramebuffers(1,C.byref(self.fbo));gl.BindFramebuffer(0x8D40,self.fbo)
            gl.FramebufferTexture2D(0x8D40,0x8CE0,GL_TEXTURE_2D,self.id,0)
            if gl.CheckFramebufferStatus(0x8D40) != 0x8CD5:
                self.close();raise ShaderError('The graphics driver cannot create this filter surface.')
            gl.ClearColor(0,0,0,1);gl.Clear(0x4000)

    def upload(self,image):
        # Native core frames are RGB32/BGRA: upload directly without conversion.
        if image.format() in (QImage.Format_RGB32,QImage.Format_ARGB32):
            fmt=0x80E1
        else:
            image=image.convertToFormat(QImage.Format_RGBA8888);fmt=0x1908
        gl=self.gl;gl.BindTexture(GL_TEXTURE_2D,self.id)
        gl.PixelStorei(0x0CF5,4);gl.PixelStorei(0x0CF2,image.bytesPerLine()//4)
        gl.TexSubImage2D(GL_TEXTURE_2D,0,0,0,*self.size,fmt,0x1401,int(VoidPtr(image.constBits())))
        gl.PixelStorei(0x0CF2,0);self.mip_dirty=True

    def close(self):
        if self.fbo.value: self.gl.DeleteFramebuffers(1,C.byref(self.fbo));self.fbo=U()
        if self.id.value: self.gl.DeleteTextures(1,C.byref(self.id));self.id=U()


class Program:
    def __init__(self,gl,vertex,fragment):
        self.gl=gl;self.id=gl.CreateProgram();shaders=[]
        try:
            for kind,source in ((0x8B31,vertex),(0x8B30,fragment)):
                shader=gl.CreateShader(kind);shaders.append(shader)
                text=C.c_char_p(source.encode());gl.ShaderSource(shader,1,C.byref(text),None)
                gl.CompileShader(shader);ok=I();gl.GetShaderiv(shader,0x8B81,C.byref(ok))
                if not ok.value:
                    log=C.create_string_buffer(16384);gl.GetShaderInfoLog(shader,len(log),None,log)
                    raise ShaderError(log.value.decode(errors='replace'))
                gl.AttachShader(self.id,shader)
            for name,index in ((b'Position',0),(b'VertexCoord',0),(b'TexCoord',1),(b'COLOR',2)):
                gl.BindAttribLocation(self.id,index,name)
            gl.LinkProgram(self.id);ok=I();gl.GetProgramiv(self.id,0x8B82,C.byref(ok))
            if not ok.value:
                log=C.create_string_buffer(16384);gl.GetProgramInfoLog(self.id,len(log),None,log)
                raise ShaderError(log.value.decode(errors='replace'))
            count=I();gl.GetProgramiv(self.id,0x8B86,C.byref(count));self.uniforms=[]
            for index in range(count.value):
                name=C.create_string_buffer(512);kind=U();size=I()
                gl.GetActiveUniform(self.id,index,512,None,C.byref(size),C.byref(kind),name)
                self.uniforms.append((name.value.decode(),kind.value,gl.GetUniformLocation(self.id,name.value)))
        except Exception:
            self.close();raise
        finally:
            for shader in shaders:gl.DeleteShader(shader)

    def close(self):
        if self.id:self.gl.DeleteProgram(self.id);self.id=0


PRESENT_VERTEX='''#version 330
in vec4 Position; in vec2 TexCoord; out vec2 uv;
void main(){gl_Position=vec4(Position.xy*2.0-1.0,0,1);uv=TexCoord;}'''
PRESENT_FRAGMENT='''#version 330
uniform sampler2D Source; in vec2 uv; out vec4 FragColor;
void main(){FragColor=vec4(texture(Source,vec2(uv.x,1.0-uv.y)).rgb,1.0);}'''


class ShaderRenderer:
    """Requires a current desktop GL 3.3 context, owned by the display widget."""
    def __init__(self,get_proc=None):
        if get_proc is None:
            from PySide6.QtGui import QOpenGLContext
            context=QOpenGLContext.currentContext()
            if context is None:raise ShaderError('No current OpenGL display context.')
            get_proc=context.getProcAddress
        self.gl=GL(get_proc);self.gl.load();gl=self.gl
        self.renderer=(gl.GetString(0x1F01) or b'Unknown renderer').decode(errors='replace')
        self.vendor=(gl.GetString(0x1F00) or b'').decode(errors='replace')
        self.hardware=not is_software_renderer(self.renderer)
        limit=I();gl.GetIntegerv(0x0D33,C.byref(limit));self.max_size=min(8192,limit.value)
        gl.GetIntegerv(0x8872,C.byref(limit));self.max_units=limit.value
        self.vao=U();self.vbo=U();self.samplers={};self.programs=[];self.outputs=[];self.history=[];self.luts={}
        self.key=None;self.cache_key=None;self.input_key=None;self.frames=0;self.output=None
        self.feedback=set();self.history_count=0;self.phase=0
        self.present=None
        try:
            gl.GenVertexArrays(1,C.byref(self.vao));gl.BindVertexArray(self.vao)
            gl.GenBuffers(1,C.byref(self.vbo));gl.BindBuffer(0x8892,self.vbo)
            values=(F*16)(0,0,0,0,1,0,1,0,0,1,0,1,1,1,1,1)
            gl.BufferData(0x8892,C.sizeof(values),values,0x88E4)
            for location,offset in ((0,0),(1,8)):
                gl.EnableVertexAttribArray(location);gl.VertexAttribPointer(location,2,0x1406,0,16,P(offset))
            gl.DisableVertexAttribArray(2);gl.VertexAttrib4f(2,1,1,1,1)
            self.present=Program(gl,PRESENT_VERTEX,PRESENT_FRAGMENT)
        except Exception:
            self.close();raise
        finally:gl.BindVertexArray(0)

    def sampler(self,linear=False,wrap='clamp_to_edge',mipmap=False):
        key=(linear,wrap,mipmap)
        if key not in self.samplers:
            item=U();gl=self.gl;gl.GenSamplers(1,C.byref(item))
            gl.SamplerParameteri(item,0x2801,(0x2703 if linear else 0x2700) if mipmap else (0x2601 if linear else 0x2600))
            gl.SamplerParameteri(item,0x2800,0x2601 if linear else 0x2600)
            for direction in (0x2802,0x2803):
                gl.SamplerParameteri(item,direction,{'repeat':0x2901,'mirrored_repeat':0x8370,'clamp_to_border':0x812D}.get(wrap,0x812F))
            self.samplers[key]=item
        return self.samplers[key]

    def clear_chain(self):
        for p in self.programs:p.close()
        for group in self.outputs:
            for t in group:t.close()
        for t in self.history:t.close()
        for t in self.luts.values():t.close()
        self.programs=[];self.outputs=[];self.history=[];self.luts={}
        self.key=None;self.cache_key=None;self.input_key=None;self.output=None;self.frames=0;self.phase=0

    def select(self,key):
        if self.key==key:return
        self.clear_chain();self.info=preset(key)
        try:
            dialect=PRESETS[key]['format']
            for spec in self.info['passes']:
                self.programs.append(Program(self.gl,stage_source(spec['source'],'vertex',dialect),stage_source(spec['source'],'fragment',dialect)))
            uniforms=[name.rsplit('.',1)[-1] for p in self.programs for name,_,_ in p.uniforms]
            self.history_count=max([int(m[1]) for n in uniforms if (m:=re.fullmatch(r'OriginalHistory(\d+)',n))]+[0])
            self.feedback={int(m[1]) for n in uniforms if (m:=re.fullmatch(r'PassFeedback(\d+)',n))}
            for i,p in enumerate(self.info['passes']):
                if p['alias'] and p['alias']+'Feedback' in uniforms:self.feedback.add(i)
            for name,spec in self.info['textures'].items():
                image=QImage(str(spec['path']))
                if image.isNull():raise ShaderError('Missing filter texture: '+spec['path'].name)
                t=Texture(self.gl,(image.width(),image.height()));self.luts[name]=t;t.upload(image)
            self.key=key
        except Exception:
            self.clear_chain();raise

    def prepare_outputs(self,source,viewport):
        sizes=[];size=source;total=0
        for i,spec in enumerate(self.info['passes']):
            size=tuple(max(1,round(spec['scale_'+axis]*({'source':size[n],'viewport':viewport[n],'absolute':1}[spec['type_'+axis]]))) for n,axis in enumerate(('x','y')))
            if max(size)>self.max_size:raise ShaderError('This filter exceeds the graphics driver’s texture size limit. Try a smaller window.')
            internal=0x881A if spec['float'] else 0x8C43 if spec['srgb'] else 0x8058
            count=2 if i in self.feedback else 1
            total+=size[0]*size[1]*(8 if spec['float'] else 4)*count
            sizes.append((size,internal,count))
        if total>512*1024*1024:raise ShaderError('This filter needs too much video memory at this window size. Try a smaller window or a lighter filter.')
        if len(self.outputs)==len(sizes) and all(group[0].size==size and group[0].internal==internal for group,(size,internal,_) in zip(self.outputs,sizes)):return
        for group in self.outputs:
            for t in group:t.close()
        self.outputs=[]
        for size,internal,count in sizes:
            group=[];self.outputs.append(group)
            for _ in range(count):group.append(Texture(self.gl,size,internal,True))
        self.cache_key=None

    def bind_texture(self,unit,texture,spec):
        gl=self.gl;gl.ActiveTexture(0x84C0+unit);gl.BindTexture(GL_TEXTURE_2D,texture.id)
        if spec.get('mipmap') and texture.mip_dirty:
            gl.GenerateMipmap(GL_TEXTURE_2D);texture.mip_dirty=False
        gl.BindSampler(unit,self.sampler(spec.get('linear',False),spec.get('wrap','clamp_to_edge'),spec.get('mipmap',False)))

    def draw_pass(self,program,textures,values,spec,legacy=False):
        gl=self.gl;gl.UseProgram(program.id);unit=0
        for full,kind,location in program.uniforms:
            name=full.rsplit('.',1)[-1]
            if kind==0x8B5E: # sampler2D
                if name not in textures:raise ShaderError('Unsupported filter input: '+name)
                if unit>=self.max_units:raise ShaderError('This filter requires more texture inputs than your graphics driver supports.')
                self.bind_texture(unit,textures[name],self.info['textures'].get(name,spec))
                gl.Uniform1i(location,unit);unit+=1
            elif kind==0x8B5C:gl.UniformMatrix4fv(location,1,0,MVP)
            elif name in values:
                value=values[name]
                if kind==0x1406:gl.Uniform1f(location,float(value))
                elif kind==0x1405:gl.Uniform1ui(location,int(value))
                elif kind in (0x1404,0x8B56):gl.Uniform1i(location,int(value))
                elif kind==0x8B50:gl.Uniform2f(location,*value[:2])
                elif kind==0x8B52:gl.Uniform4f(location,*value)
        gl.DrawArrays(0x0005,0,4)
        for i in range(unit):gl.BindSampler(i,0)

    def render(self,frame,size,key,saved=None):
        """Render into a cached GPU texture; returns a Texture, never a QImage."""
        if frame.isNull():return None
        self.select(key);gl=self.gl
        viewport=(size.width(),size.height()) if hasattr(size,'width') else tuple(size)
        if min(viewport)<1:return None
        minimum=PRESETS[key].get('minimum_viewport',(1,1))
        scale=max(1,minimum[0]/viewport[0],minimum[1]/viewport[1])
        viewport=tuple(round(side*scale) for side in viewport)
        source=(frame.width(),frame.height())
        values=parameter_values(key,saved)
        cache=(frame.cacheKey(),viewport,key,tuple(values.items()))
        if cache==self.cache_key:return self.output
        for state in (0x0BE2,0x0B71,0x0B44,0x0C11,0x0B90):gl.Disable(state)
        gl.ColorMask(1,1,1,1);gl.ActiveTexture(0x84C0)
        new_frame=frame.cacheKey()!=self.input_key
        if not self.history or self.history[0].size!=source:
            for t in self.history:t.close()
            self.history=[]
            for _ in range(self.history_count+1):
                texture=Texture(gl,source);self.history.append(texture);texture.upload(frame)
            new_frame=True
        elif new_frame:
            self.history.insert(0,self.history.pop());self.history[0].upload(frame)
        if new_frame:self.frames+=1;self.phase=1-self.phase
        self.input_key=frame.cacheKey()
        if key in ('nearest','linear'):
            self.output=self.history[0];self.cache_key=cache
            return self.output
        self.prepare_outputs(source,viewport)
        gl.BindVertexArray(self.vao)
        original=self.history[0];previous=original
        textures={'Original':original,**self.luts}
        textures.update({'OriginalHistory'+str(i):t for i,t in enumerate(self.history)})
        for i,group in enumerate(self.outputs):
            if i in self.feedback:
                textures['PassFeedback'+str(i)]=group[1-self.phase]
                alias=self.info['passes'][i]['alias']
                if alias:textures[alias+'Feedback']=group[1-self.phase]
        try:
            for i,(program,spec,group) in enumerate(zip(self.programs,self.info['passes'],self.outputs)):
                output=group[self.phase if len(group)==2 else 0]
                gl.BindFramebuffer(0x8D40,output.fbo);gl.Viewport(0,0,*output.size)
                (gl.Enable if spec['srgb'] else gl.Disable)(0x8DB9)
                textures['Source']=textures['Texture']=previous
                uniforms=dict(values,FrameCount=(self.frames-1) % spec['mod'] if spec['mod'] else self.frames-1,FrameDirection=1,FrameTimeDelta=16667,OriginalAspect=source[0]/source[1],OriginalAspectRotated=source[0]/source[1],Rotation=0,TotalSubFrames=1,CurrentSubFrame=1)
                def dimensions(dim):return (*dim,1/dim[0],1/dim[1])
                uniforms.update({name+'Size':dimensions(t.size) for name,t in textures.items()})
                uniforms.update(SourceSize=dimensions(previous.size),OutputSize=dimensions(output.size),FinalViewportSize=dimensions(viewport),InputSize=dimensions(previous.size),TextureSize=dimensions(previous.size))
                self.draw_pass(program,textures,uniforms,spec,PRESETS[key]['format']=='glsl')
                output.mip_dirty=True;textures['PassOutput'+str(i)]=output
                if spec['alias']:textures[spec['alias']]=output
                previous=output
            self.output=previous;self.cache_key=cache
            return previous
        finally:
            gl.Disable(0x8DB9);gl.BindVertexArray(0);gl.UseProgram(0);gl.ActiveTexture(0x84C0)

    def display(self,frame,size,key,saved,framebuffer,viewport):
        """Present to Qt's default FBO (which need not be zero)."""
        output=self.render(frame,size,key,saved)
        gl=self.gl;gl.BindFramebuffer(0x8D40,framebuffer)
        gl.Disable(0x8DB9);gl.Disable(0x0BE2);gl.Disable(0x0C11)
        gl.ClearColor(.047,.051,.063,1);gl.Clear(0x4000)
        if output:
            gl.Viewport(*viewport);gl.BindVertexArray(self.vao);gl.UseProgram(self.present.id)
            self.bind_texture(0,output,{'linear':key=='linear' or (key!='nearest' and output.size!=viewport[2:])})
            gl.Uniform1i(gl.GetUniformLocation(self.present.id,b'Source'),0)
            gl.DrawArrays(0x0005,0,4)
        gl.BindSampler(0,0);gl.BindVertexArray(0);gl.UseProgram(0);gl.ActiveTexture(0x84C0)

    def close(self):
        self.clear_chain()
        if self.present:self.present.close();self.present=None
        for sampler in self.samplers.values():self.gl.DeleteSamplers(1,C.byref(sampler))
        self.samplers.clear()
        if self.vbo.value:self.gl.DeleteBuffers(1,C.byref(self.vbo));self.vbo=U()
        if self.vao.value:self.gl.DeleteVertexArrays(1,C.byref(self.vao));self.vao=U()
