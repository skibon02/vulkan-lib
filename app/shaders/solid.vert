#version 450

layout(location = 0) in vec3 pos;
layout(location = 1) in vec3 col;

layout(location = 0) out vec3 fragColor;

layout(binding = 0) uniform Color {
    int data[4];
} color;

void main() {
    gl_Position = vec4(pos, 1.0);
    fragColor = col;
}