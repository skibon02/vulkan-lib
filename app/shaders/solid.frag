#version 450 core

layout(location = 0) in vec2 texCoord;

layout(location = 0) out vec4 outColor;

layout(binding = 1) uniform sampler2D tex;

void main() {
    vec4 v = texture(tex, texCoord);
    v.a = 1.0;
    outColor = v;
}