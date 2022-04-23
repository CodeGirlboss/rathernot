const _service = (thing) => console.log(thing)

async function attemptToCall (props, args) {
  var ptr = registeredObjects[props[0]];
  for (var i =1; i < props.length; i++) {
    if (ptr[props[i]]) {
      ptr = ptr[props[i]];
    } else {
      throw new Error(`I can't find property ${props[i]} on object ${JSON.stringify(ptr, null, 2)}`);
    }
  }
  return ptr(...args);
}

const registeredObjects = {
  //key: serviceId
  //value: registered object
}

_service._RNOS_SERVER = (id, thing) => {
  registeredObjects[id] = thing;
  return thing;
};

_service._call = attemptToCall;

export const Service = _service;

export const URel = {
  toString: () => window.location.href,
  moveTo: (segment) => {console.log('tried to move')}
}

