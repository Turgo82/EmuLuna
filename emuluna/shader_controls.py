"""In-game preset selection and live, console-specific shader parameters."""
import math
from PySide6.QtCore import Qt
from PySide6.QtWidgets import (QDialog,QVBoxLayout,QLabel,QFormLayout,QDoubleSpinBox,
    QDialogButtonBox,QScrollArea,QWidget,QComboBox,QFrame,QGridLayout)
from .shaders import PRESETS,parameters


class ShaderControls(QDialog):
    def __init__(self,key,values,changed,parent=None,*,select=None,backend='',resolution=0,resolution_changed=None):
        super().__init__(parent)
        self.setWindowTitle('Configure Shader')
        self.setMinimumWidth(460)
        self.setAttribute(Qt.WA_DeleteOnClose)
        self.changed=changed;self.select=select
        layout=QVBoxLayout(self);layout.setContentsMargins(18,18,18,18);layout.setSpacing(12)
        self.selector=QComboBox()
        for shader,spec in PRESETS.items():
            self.selector.addItem(spec['name'],shader)
        self.selector.setCurrentIndex(self.selector.findData(key))
        self.selector.setAccessibleName('Video filter')
        form=QFormLayout();form.addRow('Filter',self.selector);layout.addLayout(form)
        self.details=QLabel();self.details.setWordWrap(True);layout.addWidget(self.details)
        self.backend=QLabel(backend);self.backend.setWordWrap(True);self.backend.setTextInteractionFlags(Qt.TextSelectableByMouse)
        layout.addWidget(self.backend)
        self.resolution=QComboBox()
        for label,value in [('Match window',0),('Up to 1080p',1080),('Up to 720p',720)]:self.resolution.addItem(label,value)
        self.resolution.setCurrentIndex(max(0,self.resolution.findData(resolution)))
        self.resolution.setToolTip('Lower the filter resolution to reduce GPU work in large windows. The game resolution and screenshots stay unchanged.')
        form=QFormLayout();form.addRow('Filter resolution',self.resolution);layout.addLayout(form)
        if resolution_changed:self.resolution.currentIndexChanged.connect(lambda _:resolution_changed(self.resolution.currentData()))
        self.scroll=QScrollArea();self.scroll.setWidgetResizable(True);self.scroll.setFrameShape(QFrame.StyledPanel)
        layout.addWidget(self.scroll,1)
        self.buttons=QDialogButtonBox(QDialogButtonBox.Close|QDialogButtonBox.RestoreDefaults)
        self.buttons.rejected.connect(self.close)
        self.buttons.button(QDialogButtonBox.RestoreDefaults).clicked.connect(self.restore)
        layout.addWidget(self.buttons)
        self.populate(key,values)
        self.selector.currentIndexChanged.connect(self.switch)
        if not select:self.selector.setEnabled(False)
        self.resize(570,620)

    def set_backend(self,text):self.backend.setText(text)

    def populate(self,key,values):
        spec=PRESETS[key]
        self.details.setText(f"{spec['passes']} render pass{'es' if spec['passes']!=1 else ''} · {spec['cost']} GPU demand"+
            ('\nFor slower graphics hardware, try zfast CRT or lower the filter resolution.' if spec['cost']=='High' else '')+
            ('\n'+spec['note'] if spec.get('note') else ''))
        content=QWidget();form=QGridLayout(content);form.setContentsMargins(12,12,12,12)
        form.setColumnStretch(0,1);form.setAlignment(Qt.AlignTop)
        form.setVerticalSpacing(10)
        self.fields={};self.specs=parameters(key)
        for row,item in enumerate(self.specs):
            field=QDoubleSpinBox();field.setFixedWidth(125);field.setRange(item['minimum'],item['maximum'])
            field.setSingleStep(item['step']);field.setDecimals(max(0,min(5,math.ceil(-math.log10(item['step'])))))
            field.setValue(values[item['key']]);field.setAccessibleName(item['label']);field.setKeyboardTracking(False)
            field.valueChanged.connect(lambda value,name=item['key']:self.changed(name,value))
            label=QLabel(item['label']);label.setWordWrap(True)
            form.addWidget(label,row,0);form.addWidget(field,row,1);self.fields[item['key']]=field
        if not self.specs:
            label=QLabel('This filter has no adjustable parameters.');label.setWordWrap(True);form.addWidget(label,0,0,1,2)
        self.scroll.setWidget(content)
        self.buttons.button(QDialogButtonBox.RestoreDefaults).setEnabled(bool(self.specs))

    def switch(self):
        key=self.selector.currentData()
        values=self.select(key)
        self.populate(key,values)

    def restore(self):
        for spec in self.specs:self.fields[spec['key']].setValue(spec['default'])
