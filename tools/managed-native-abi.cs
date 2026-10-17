// Build-time metadata inspection. No Assembly.Load, dependency resolution or native execution.
using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.IO;
using System.Reflection;
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;

public sealed class ManagedAbiMethod
{
    public string Owner;
    public string Name;
    public string EntryPoint;
    public string Library;
    public int ImportFlags;
    public bool IsStatic;
    public bool IsPrivate;
    public bool PreserveSignature;
    public bool IsFixedSignature;
    public string Returns;
    public int ReturnMarshal;
    public string[] Parameters;
    public int[] ParameterFlags;
    public int[] ParameterMarshal;
}

public sealed class ManagedAbiCallback
{
    public string Type;
    public int CallingConvention;
    public ManagedAbiMethod Invoke;
}

public sealed class ManagedAbiAssembly
{
    public string Name;
    public ManagedAbiMethod[] Imports;
    public ManagedAbiCallback[] Callbacks;
}

public static class ManagedNativeAbiMetadata
{
    private sealed class Types : ISignatureTypeProvider<string, object>
    {
        public string GetArrayType(string element, ArrayShape shape) { return element + "[rank=" + shape.Rank + "]"; }
        public string GetByReferenceType(string element) { return element + "&"; }
        public string GetPointerType(string element) { return element + "*"; }
        public string GetSZArrayType(string element) { return element + "[]"; }
        public string GetPinnedType(string element) { return "pinned:" + element; }
        public string GetModifiedType(string modifier, string element, bool required) { return "modified:" + element; }
        public string GetGenericMethodParameter(object context, int index) { return "method-generic:" + index; }
        public string GetGenericTypeParameter(object context, int index) { return "type-generic:" + index; }
        public string GetGenericInstantiation(string generic, ImmutableArray<string> arguments) { return "generic:" + generic; }
        public string GetFunctionPointerType(MethodSignature<string> signature) { return "function-pointer"; }
        public string GetPrimitiveType(PrimitiveTypeCode code) { return "System." + code; }
        public string GetTypeFromDefinition(MetadataReader reader, TypeDefinitionHandle handle, byte kind)
        {
            TypeDefinition type = reader.GetTypeDefinition(handle);
            string name = reader.GetString(type.Name);
            TypeDefinitionHandle parent = type.GetDeclaringType();
            return parent.IsNil ? Join(reader.GetString(type.Namespace), name) :
                GetTypeFromDefinition(reader, parent, kind) + "+" + name;
        }
        public string GetTypeFromReference(MetadataReader reader, TypeReferenceHandle handle, byte kind)
        {
            TypeReference type = reader.GetTypeReference(handle);
            string name = reader.GetString(type.Name);
            return type.ResolutionScope.Kind == HandleKind.TypeReference ?
                GetTypeFromReference(reader, (TypeReferenceHandle)type.ResolutionScope, kind) + "+" + name :
                Join(reader.GetString(type.Namespace), name);
        }
        public string GetTypeFromSpecification(MetadataReader reader, object context, TypeSpecificationHandle handle, byte kind)
        {
            return reader.GetTypeSpecification(handle).DecodeSignature(this, context);
        }
        private static string Join(string ns, string name) { return ns.Length == 0 ? name : ns + "." + name; }
    }

    private static readonly Types TypeNames = new Types();

    private static string Name(MetadataReader reader, EntityHandle handle)
    {
        if (handle.Kind == HandleKind.TypeDefinition)
            return TypeNames.GetTypeFromDefinition(reader, (TypeDefinitionHandle)handle, 0);
        if (handle.Kind == HandleKind.TypeReference)
            return TypeNames.GetTypeFromReference(reader, (TypeReferenceHandle)handle, 0);
        throw new InvalidDataException("Unsupported ABI metadata type handle.");
    }

    private static string AttributeType(MetadataReader reader, CustomAttribute attribute)
    {
        EntityHandle constructor = attribute.Constructor;
        if (constructor.Kind == HandleKind.MemberReference)
            return Name(reader, reader.GetMemberReference((MemberReferenceHandle)constructor).Parent);
        if (constructor.Kind == HandleKind.MethodDefinition)
            return Name(reader, reader.GetMethodDefinition((MethodDefinitionHandle)constructor).GetDeclaringType());
        throw new InvalidDataException("Unsupported attribute constructor.");
    }

    private static int MarshalType(MetadataReader reader, BlobHandle descriptor)
    {
        if (descriptor.IsNil) return -1;
        BlobReader blob = reader.GetBlobReader(descriptor);
        // Primitive ABI declarations need either the default descriptor or exactly I1.
        if (blob.Length != 1) return -2;
        return blob.ReadByte();
    }

    private static ManagedAbiMethod Method(MetadataReader reader, MethodDefinitionHandle handle)
    {
        MethodDefinition method = reader.GetMethodDefinition(handle);
        MethodSignature<string> signature = method.DecodeSignature(TypeNames, (object)null);
        if (signature.GenericParameterCount != 0 || signature.ParameterTypes.Length > 32)
            throw new InvalidDataException("Generic or oversized ABI method signature.");
        var flags = new int[signature.ParameterTypes.Length];
        var marshals = new int[signature.ParameterTypes.Length];
        for (int i = 0; i < marshals.Length; i++) marshals[i] = -1;
        int returnMarshal = -1;
        foreach (ParameterHandle parameterHandle in method.GetParameters())
        {
            Parameter parameter = reader.GetParameter(parameterHandle);
            if (parameter.SequenceNumber == 0)
                returnMarshal = MarshalType(reader, parameter.GetMarshallingDescriptor());
            else
            {
                int index = parameter.SequenceNumber - 1;
                if (index >= flags.Length) throw new InvalidDataException("Invalid ABI parameter sequence.");
                flags[index] = (int)parameter.Attributes;
                marshals[index] = MarshalType(reader, parameter.GetMarshallingDescriptor());
            }
        }
        var parameters = new string[signature.ParameterTypes.Length];
        signature.ParameterTypes.CopyTo(parameters);
        return new ManagedAbiMethod
        {
            Owner = Name(reader, method.GetDeclaringType()), Name = reader.GetString(method.Name),
            IsStatic = (method.Attributes & MethodAttributes.Static) != 0,
            IsPrivate = (method.Attributes & MethodAttributes.MemberAccessMask) == MethodAttributes.Private,
            PreserveSignature = (method.ImplAttributes & MethodImplAttributes.PreserveSig) != 0,
            IsFixedSignature = signature.Header.CallingConvention == SignatureCallingConvention.Default,
            Returns = signature.ReturnType, ReturnMarshal = returnMarshal, Parameters = parameters,
            ParameterFlags = flags, ParameterMarshal = marshals
        };
    }

    public static ManagedAbiAssembly Read(string path)
    {
        using (var stream = File.OpenRead(path))
        using (var pe = new PEReader(stream))
        {
            if (stream.Length > 64 * 1024 * 1024 || !pe.HasMetadata ||
                pe.PEHeaders.CoffHeader.Machine != Machine.Amd64)
                throw new InvalidDataException("Expected bounded win-x64 managed assembly metadata.");
            MetadataReader reader = pe.GetMetadataReader();
            if (!reader.IsAssembly || reader.TypeDefinitions.Count > 20000 || reader.MethodDefinitions.Count > 200000)
                throw new InvalidDataException("Invalid or oversized managed ABI metadata.");
            var imports = new List<ManagedAbiMethod>();
            var callbacks = new List<ManagedAbiCallback>();
            foreach (TypeDefinitionHandle typeHandle in reader.TypeDefinitions)
            {
                TypeDefinition type = reader.GetTypeDefinition(typeHandle);
                int convention = -1;
                foreach (CustomAttributeHandle attributeHandle in type.GetCustomAttributes())
                {
                    CustomAttribute attribute = reader.GetCustomAttribute(attributeHandle);
                    if (AttributeType(reader, attribute) != "System.Runtime.InteropServices.UnmanagedFunctionPointerAttribute") continue;
                    BlobReader value = reader.GetBlobReader(attribute.Value);
                    if (convention != -1 || value.Length != 8 || value.ReadUInt16() != 1)
                        throw new InvalidDataException("Invalid or unsupported callback attribute.");
                    convention = value.ReadInt32();
                    if (value.ReadUInt16() != 0) throw new InvalidDataException("Callback named arguments require review.");
                }
                foreach (MethodDefinitionHandle methodHandle in type.GetMethods())
                {
                    MethodDefinition method = reader.GetMethodDefinition(methodHandle);
                    if ((method.Attributes & MethodAttributes.PinvokeImpl) != 0)
                    {
                        MethodImport import = method.GetImport();
                        string library = reader.GetString(reader.GetModuleReference(import.Module).Name);
                        string entry = reader.GetString(import.Name);
                        string methodName = reader.GetString(method.Name);
                        if (library != "quicklook_next_native" && library != "quicklook_next_native.dll" &&
                            !entry.StartsWith("ql_", StringComparison.Ordinal) && !methodName.StartsWith("ql_", StringComparison.Ordinal)) continue;
                        ManagedAbiMethod decoded = Method(reader, methodHandle);
                        decoded.Library = library; decoded.EntryPoint = entry; decoded.ImportFlags = (int)import.Attributes;
                        imports.Add(decoded);
                    }
                    if (convention != -1 && reader.GetString(method.Name) == "Invoke")
                    {
                        if (type.BaseType.IsNil || Name(reader, type.BaseType) != "System.MulticastDelegate")
                            throw new InvalidDataException("Unmanaged callback must be a delegate.");
                        callbacks.Add(new ManagedAbiCallback { Type = Name(reader, typeHandle),
                            CallingConvention = convention, Invoke = Method(reader, methodHandle) });
                    }
                }
            }
            return new ManagedAbiAssembly { Name = reader.GetString(reader.GetAssemblyDefinition().Name),
                Imports = imports.ToArray(), Callbacks = callbacks.ToArray() };
        }
    }
}
