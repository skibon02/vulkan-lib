#version 450

// Per-instance attributes (binding 0, per instance)
layout(location = 0) in vec3 pos;      // top-left corner position
layout(location = 1) in vec2 size;     // width and height
layout(location = 2) in vec4 color;    // solid color

layout(location = 0) out vec4 fragColor;
layout(location = 1) out vec2 texCoord;

layout(binding = 0) uniform UniformData {
    int data[4];
} uniformData;

// 4 vertices per instance: top-left, top-right, bottom-left, bottom-right
const vec2 vertices[4] = vec2[4](
    vec2(0.0, 0.0),  // top-left
    vec2(1.0, 0.0),  // top-right
    vec2(0.0, 1.0),  // bottom-left
    vec2(1.0, 1.0)   // bottom-right
);

void main() {
    vec2 vertexOffset = vertices[gl_VertexIndex];
    vec3 vertexPos = pos + vec3(vertexOffset * size, 0.0);

    gl_Position = vec4(vertexPos, 1.0);
    fragColor = color;
    texCoord = vertexOffset;
}